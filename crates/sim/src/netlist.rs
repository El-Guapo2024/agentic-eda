//! The simulator's netlist, made from kicad-cli's.
//!
//! `kicad-cli sch export netlist --format spice` runs `NETLIST_EXPORTER_SPICE::DoWriteNetlist` with `OPTION_SIM_COMMAND` alone
//! (`EESCHEMA_JOBS_HANDLER::JobExportNetlist`): the title, the libraries (`.include`), the models, the schematic's directives -- its
//! analyses among them -- the items and `.end`. KiCad's simulator asks the same exporter for more (`SPICE_CIRCUIT_MODEL::GetNetlist` with the
//! tab's command and options): `.save all`, `.probe alli` and a `.probe p(..)` for every item before the directives, the schematic's analyses
//! left out when the tab has a command of its own, the tab's command after them, and the libraries' paths made absolute. [`simulator_netlist`]
//! makes that netlist out of kicad-cli's, line for line; nothing about the circuit is written here.
//!
//! [`circuit`] reads back what `SPICE_CIRCUIT_MODEL` knows of the circuit once it has read the schematic (`GetNets`, `GetItems`, `FindItem`,
//! `GetItemName`), from the same netlist and kicad-cli's XML one: the dialogs list nets and sources from it, the signals list is built from
//! it, and a probe or a tuner on the schematic is answered from it.

use crate::command::{is_sim_command, text_is_directive};
use crate::xml::XmlNetlist;
use std::collections::{BTreeMap, BTreeSet};

/// `NETLIST_EXPORTER_SPICE`'s options (`netlist_exporter_spice.h`), the bits a tab's `GetSimOptions` holds.
pub const OPTION_ADJUST_INCLUDE_PATHS: u32 = 0x0010;
pub const OPTION_ADJUST_PASSIVE_VALS: u32 = 0x0020;
pub const OPTION_SAVE_ALL_VOLTAGES: u32 = 0x0040;
pub const OPTION_SAVE_ALL_CURRENTS: u32 = 0x0080;
pub const OPTION_SAVE_ALL_DISSIPATIONS: u32 = 0x0100;
pub const OPTION_CUR_SHEET_AS_ROOT: u32 = 0x0200;
pub const OPTION_SIM_COMMAND: u32 = 0x0400;
pub const OPTION_SAVE_ALL_EVENTS: u32 = 0x0800;
pub const OPTION_DEFAULT_FLAGS: u32 =
    OPTION_ADJUST_INCLUDE_PATHS | OPTION_ADJUST_PASSIVE_VALS | OPTION_SAVE_ALL_VOLTAGES | OPTION_SAVE_ALL_CURRENTS | OPTION_SAVE_ALL_DISSIPATIONS | OPTION_SAVE_ALL_EVENTS;

/// One item line of the netlist: `R1 /in /out 1k`.
#[derive(Debug, Clone, PartialEq)]
pub struct NetlistItem {
    /// `SPICE_GENERATOR::ItemName`: the reference, prefixed by the device's letter when it does not start with it (`XU1` for `U1`).
    pub name: String,
    pub tokens: Vec<String>,
}

impl NetlistItem {
    /// The SPICE device letter, upper case.
    pub fn letter(&self) -> char {
        self.name.chars().next().unwrap_or(' ').to_ascii_uppercase()
    }
}

/// kicad-cli's netlist cut into its parts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetlistParts {
    /// `.title` and the `.include` lines and `.model` lines (with their `+` continuations): `WriteHead`, `writeIncludes`, `writeModels`.
    pub head: Vec<String>,
    /// The schematic's directives, each its whole text (`ReadDirectives`), as found in the netlist.
    pub directives: Vec<String>,
    /// The item lines (`writeItems`), continuations joined to their line.
    pub items: Vec<String>,
}

fn is_head_line(l: &str) -> bool {
    let low = l.trim_start().to_lowercase();
    low.starts_with(".title") || low.starts_with(".include") || low.starts_with(".model") || low.starts_with('+') || low.trim().is_empty()
}

/// Cut kicad-cli's netlist into the head, the directives and the items. `directives` are the texts of the schematic that `text_is_directive`
/// (`ReadDirectives`), in any order: the netlist has them in one block, in the exporter's order, and that order is kept. A directive that
/// is not found as it is (a text variable in it, say) leaves its lines where they are; then every dot line between the head and the first
/// item is taken as a directive of its own, so that no analysis of the schematic passes for an item.
pub fn split_netlist(cli: &str, directives: &[String]) -> NetlistParts {
    let text = cli.replace("\r\n", "\n");
    let mut body: &str = &text;
    if let Some(i) = body.rfind("\n.end") {
        body = &body[..=i];
    } else if body.starts_with(".end") {
        body = "";
    }
    // Every directive where it starts a line, earliest first.
    let mut found: Vec<(usize, usize, String)> = Vec::new();
    for d in directives.iter().filter(|d| !d.is_empty()) {
        let needle = format!("\n{d}\n");
        let mut from = 0;
        while let Some(i) = body[from..].find(&needle) {
            let at = from + i + 1;
            if !found.iter().any(|(s, e, _)| at < *e && *s < at + d.len() + 1) {
                found.push((at, at + d.len() + 1, d.clone()));
                break;
            }
            from = at;
        }
    }
    found.sort_by_key(|f| f.0);
    let (before, after) = match (found.first(), found.last()) {
        (Some(first), Some(last)) => (&body[..first.0], &body[last.1..]),
        _ => (body, ""),
    };
    let mut head = Vec::new();
    let mut loose = Vec::new();
    let mut items = Vec::new();
    let mut in_head = true;
    for line in before.lines() {
        if in_head && is_head_line(line) {
            if !line.trim().is_empty() {
                head.push(line.to_string());
            }
            continue;
        }
        in_head = false;
        if found.is_empty() && items.is_empty() && line.trim_start().starts_with('.') {
            loose.push(line.to_string());
            continue;
        }
        items.push(line.to_string());
    }
    if !found.is_empty() {
        items = after.lines().map(str::to_string).collect();
    }
    let mut joined: Vec<String> = Vec::new();
    for l in items.into_iter().filter(|l| !l.trim().is_empty()) {
        if let (Some(rest), Some(last)) = (l.trim_start().strip_prefix('+'), joined.last_mut()) {
            last.push(' ');
            last.push_str(rest.trim());
        } else {
            joined.push(l);
        }
    }
    let directives = if found.is_empty() { loose } else { found.into_iter().map(|f| f.2).collect() };
    NetlistParts { head, directives, items: joined }
}

/// The item lines as items (comments skipped).
pub fn parse_items(parts: &NetlistParts) -> Vec<NetlistItem> {
    parts
        .items
        .iter()
        .filter(|l| !l.trim_start().starts_with('*'))
        .filter_map(|l| {
            let tokens: Vec<String> = l.split_whitespace().map(str::to_string).collect();
            Some(NetlistItem { name: tokens.first()?.clone(), tokens })
        })
        .collect()
}

/// What [`simulator_netlist`] made, and what it has to say (`WX_STRING_REPORTER` of `GetNetlist`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SimNetlist {
    pub text: String,
    pub messages: Vec<String>,
}

/// `.include "<path>"` with the path as `NETLIST_EXPORTER_SPICE::writeInclude` writes it: with `OPTION_ADJUST_INCLUDE_PATHS`, made absolute
/// by `resolve` (KiCad's `ResolveFile`: the path as it is, then against the project) and, when it is not found, "Could not find library
/// file '%s'" and the path as it is.
fn include_line(line: &str, options: u32, resolve: &dyn Fn(&str) -> Option<String>, messages: &mut Vec<String>) -> String {
    let trimmed = line.trim();
    let Some(rest) = trimmed.strip_prefix(".include").or_else(|| trimmed.strip_prefix(".INCLUDE")) else { return line.to_string() };
    let path = rest.trim().trim_matches('"');
    if options & OPTION_ADJUST_INCLUDE_PATHS == 0 {
        return line.to_string();
    }
    match resolve(path) {
        Some(full) => format!(".include \"{full}\""),
        None => {
            messages.push(format!("Could not find library file '{path}'"));
            line.to_string()
        }
    }
}

/// The netlist KiCad's simulator gives ngspice for `command` with `options` (`SPICE_CIRCUIT_MODEL::GetNetlist`, `DoWriteNetlist`), made from
/// kicad-cli's `cli` netlist:
/// - `WriteHead`, `writeIncludes` (paths adjusted, above) and `writeModels` as kicad-cli wrote them;
/// - `SPICE_CIRCUIT_MODEL::WriteDirectives`, when there are items: `.save all`, `.probe alli` and `.probe p(<item>)` for every item of two
///   pins or more but XSPICE ones (`A`: "ngspice (v39) does not support power measurement for XSPICE devices") by the options, the schematic's
///   directives but its analyses when the tab has a command (an empty one keeps them, `OPTION_SIM_COMMAND`), then the command;
/// - `writeItems` and `WriteTail` as kicad-cli wrote them.
///
/// `pin_counts` gives an item's pin count (`SIM_MODEL::GetPinCount`) by item name; an item it does not name counts its nodes as two.
pub fn simulator_netlist(cli: &str, directives: &[String], command: &str, options: u32, pin_counts: &BTreeMap<String, usize>, resolve: &dyn Fn(&str) -> Option<String>) -> SimNetlist {
    let parts = split_netlist(cli, directives);
    let items = parse_items(&parts);
    let mut messages = Vec::new();
    let mut out = String::new();
    for l in &parts.head {
        out.push_str(&include_line(l, options, resolve, &mut messages));
        out.push('\n');
    }
    if !items.is_empty() {
        if options & OPTION_SAVE_ALL_VOLTAGES != 0 {
            out.push_str(".save all\n");
        }
        if options & OPTION_SAVE_ALL_CURRENTS != 0 {
            out.push_str(".probe alli\n");
        }
        if options & OPTION_SAVE_ALL_DISSIPATIONS != 0 {
            for item in &items {
                let pins = pin_counts.get(&item.name).copied().unwrap_or(2);
                if pins >= 2 && !item.name.is_empty() && !item.name.starts_with('A') {
                    out.push_str(&format!(".probe p({})\n", item.name));
                }
            }
        }
        let keep_sim_commands = command.is_empty();
        for d in &parts.directives {
            if !is_sim_command(d) || keep_sim_commands {
                out.push_str(d);
                out.push('\n');
            }
        }
        if !command.is_empty() {
            out.push_str(command);
            out.push('\n');
        }
    }
    for l in &parts.items {
        out.push_str(l);
        out.push('\n');
    }
    out.push_str(".end\n");
    SimNetlist { text: out, messages }
}

/// `NETLIST_EXPORTER_SPICE::ConvertToSpiceMarkup` of an unescaped net name: overbar markup `~{X}` becomes `~X`, sub- and superscripts lose
/// their markup, the characters ngspice does not take become '_', and a root-sheet name that starts "//" starts "/root/".
pub fn spice_net_name(name: &str) -> String {
    let mut s = String::new();
    let chars: Vec<char> = name.chars().collect();
    let mut i = 0;
    let mut depth = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if (c == '~' || c == '_' || c == '^') && chars.get(i + 1) == Some(&'{') {
            if c == '~' {
                s.push('~');
            }
            depth += 1;
            i += 2;
            continue;
        }
        if c == '}' && depth > 0 {
            depth -= 1;
            i += 1;
            continue;
        }
        s.push(c);
        i += 1;
    }
    let mut s: String = s.chars().map(|c| if matches!(c, '%' | '(' | ')' | ',' | '[' | ']' | '<' | '>' | '~' | ' ') { '_' } else { c }).collect();
    if s.starts_with("//") {
        s = s.replacen("//", "/root/", 1);
    }
    s
}

/// `Sim.Pins` (`SIM_PINS_FIELD`): "1=in+ 2=in- 3=vcc" as (symbol pin number, model pin name), in the field's order.
pub fn parse_sim_pins(field: &str) -> Vec<(String, String)> {
    field.split_whitespace().filter_map(|t| t.split_once('=')).map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

/// One item of the circuit as `SPICE_CIRCUIT_MODEL::GetItems` has it, with what the simulator asks of its model.
#[derive(Debug, Clone, PartialEq)]
pub struct CircuitItem {
    /// `SPICE_ITEM::refName`.
    pub reference: String,
    /// `SPICE_GENERATOR::ItemName`.
    pub name: String,
    /// The SPICE letter (`R`, `V`, `X`, `Q`, ...).
    pub device: char,
    /// The nets on the model's pins, in the model's order (`SPICE_ITEM::pinNetNames`).
    pub nodes: Vec<String>,
    /// `SIM_MODEL::GetPinCount`.
    pub pin_count: usize,
    /// Symbol pin number -> model pin index (`SIM_MODEL::FindModelPinIndex`).
    pub pin_index: BTreeMap<String, usize>,
    /// `SPICE_GENERATOR::CurrentNames`, in the model's pin order where there is one per pin.
    pub currents: Vec<String>,
    /// The tuner's parameter and its value (`SIM_MODEL::GetTunerParam`): `r`, `c` or `l` of an ideal passive, `dc` of a DC source.
    pub tuner: Option<(String, String)>,
    /// The item line as the netlist has it.
    pub line: String,
}

/// The ports of the subcircuits the netlist's libraries define (`.subckt name port ... [params:]`), by name, lower case.
pub fn subckt_ports(library_text: &str) -> BTreeMap<String, Vec<String>> {
    let mut m = BTreeMap::new();
    for l in library_text.lines() {
        let t: Vec<&str> = l.split_whitespace().collect();
        if t.first().is_some_and(|w| w.eq_ignore_ascii_case(".subckt")) && t.len() >= 2 {
            let ports: Vec<String> = t[2..].iter().take_while(|w| !w.contains('=') && !w.eq_ignore_ascii_case("params:")).map(|w| w.to_string()).collect();
            m.insert(t[1].to_lowercase(), ports);
        }
    }
    m
}

/// `SIM_MODEL_NGSPICE`'s pin names by device letter (`ModelInfo().pinNames`), the order its `CurrentNames` follow.
fn fixed_pins(letter: char) -> Option<&'static [&'static str]> {
    match letter {
        'Q' => Some(&["c", "b", "e", "s"]),
        'M' => Some(&["d", "g", "s", "b"]),
        'J' | 'Z' => Some(&["d", "g", "s"]),
        _ => None,
    }
}

/// The circuit (`SPICE_CIRCUIT_MODEL` after `ReadSchematicAndLibraries`): its items, from kicad-cli's SPICE netlist (`parts`) and its XML
/// netlist (`xml`: the symbols' fields and pins), and the ports of the subcircuits its libraries define (`ports`).
pub fn circuit(parts: &NetlistParts, xml: &XmlNetlist, ports: &BTreeMap<String, Vec<String>>) -> Vec<CircuitItem> {
    let mut out = Vec::new();
    for item in parse_items(parts) {
        let letter = item.letter();
        // The reference: the item's name, or the name without the letter KiCad put before it.
        let comp = xml.comp(&item.name).or_else(|| item.name.get(1..).and_then(|r| xml.comp(r)));
        let reference = comp.map(|c| c.reference.clone()).unwrap_or_else(|| item.name.clone());
        let sim_pins = comp.and_then(|c| c.fields.get("Sim.Pins")).map(|f| parse_sim_pins(f)).unwrap_or_default();
        // The symbol's pins sorted by number, as `InferSimModel` maps them.
        let mut symbol_pins: Vec<String> = comp.map(|c| c.pins.clone()).unwrap_or_default();
        symbol_pins.sort();
        symbol_pins.dedup();
        let subckt = (letter == 'X').then(|| item.tokens.iter().skip(1).find_map(|t| ports.get(&t.to_lowercase()))).flatten();
        // The model's pin names, in the model's order.
        let model_pins: Vec<String> = if let Some(p) = subckt {
            p.clone()
        } else if let Some(f) = fixed_pins(letter) {
            let n = if sim_pins.is_empty() { symbol_pins.len() } else { sim_pins.len() };
            f.iter().take(n.max(3)).map(|s| s.to_string()).collect()
        } else if !sim_pins.is_empty() {
            sim_pins.iter().map(|(_, m)| m.clone()).collect()
        } else if matches!(letter, 'A' | 'K') {
            Vec::new()
        } else {
            // An ideal passive or a DC source inferred from its value: "+" and "-" on the symbol's two pins.
            vec!["+".to_string(), "-".to_string()]
        };
        let pin_count = if letter == 'K' { 0 } else { model_pins.len() };
        let nodes: Vec<String> = item.tokens.iter().skip(1).take(pin_count).cloned().collect();
        let mut pin_index = BTreeMap::new();
        if sim_pins.is_empty() {
            for (i, p) in symbol_pins.iter().enumerate().take(pin_count) {
                pin_index.insert(p.clone(), i);
            }
        } else {
            for (sym, model) in &sim_pins {
                if let Some(i) = model_pins.iter().position(|m| m.eq_ignore_ascii_case(model)) {
                    pin_index.insert(sym.clone(), i);
                }
            }
        }
        let currents: Vec<String> = match letter {
            'A' | 'K' => Vec::new(),
            'Q' => ["c", "b", "e"].iter().map(|p| format!("I({}:{p})", item.name)).collect(),
            'M' | 'J' | 'Z' => ["d", "g", "s"].iter().map(|p| format!("I({}:{p})", item.name)).collect(),
            'X' if pin_count != 2 => model_pins.iter().map(|p| format!("I({}:{p})", item.name)).collect(),
            _ => vec![format!("I({})", item.name)],
        };
        let rest: Vec<&str> = item.tokens.iter().skip(1 + pin_count).map(String::as_str).collect();
        let tuner = match letter {
            'R' | 'C' | 'L' if rest.len() == 1 && crate::value::SpiceValue::parse(rest[0]).is_some() => Some((letter.to_ascii_lowercase().to_string(), rest[0].to_string())),
            'V' | 'I' if rest.first().is_some_and(|t| t.eq_ignore_ascii_case("dc")) && rest.len() >= 2 && rest.iter().skip(2).all(|t| !t.contains('(')) => Some(("dc".to_string(), rest[1].to_string())),
            _ => None,
        };
        out.push(CircuitItem { reference, name: item.name.clone(), device: letter, nodes, pin_count, pin_index, currents, tuner, line: item.tokens.join(" ") });
    }
    out
}

/// `SPICE_CIRCUIT_MODEL::GetNets`: every net on an item's pin.
pub fn circuit_nets(items: &[CircuitItem]) -> Vec<String> {
    let set: BTreeSet<&String> = items.iter().flat_map(|i| i.nodes.iter()).collect();
    set.into_iter().cloned().collect()
}

/// The texts of the schematic that are directives, in order (`NETLIST_EXPORTER_SPICE::ReadDirectives` over `texts`, the sheets' texts and text
/// boxes in the exporter's sheet order, each with its `\r\n` and `\n` as they are).
pub fn read_directives<'a>(texts: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    texts.into_iter().filter(|t| text_is_directive(t)).map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::{Comp, XmlNet};

    const CLI: &str = ".title KiCad schematic\n.include \"/usr/share/kicad/symbols/Simulation_SPICE.sp\"\n.include \"models/my.lib\"\n.save v(out)\n.tran 1u 1m\nXU1 GND /INV VCC /VEE /OUT kicad_builtin_opamp\nR2 /INV /OUT 100k\nV1 /IN GND DC 0 SIN( 0 1 1k 0 0 0 ) AC 1\n.end\n";

    fn directives() -> Vec<String> {
        vec![".save v(out)".into(), ".tran 1u 1m".into()]
    }

    fn resolve(p: &str) -> Option<String> {
        if p.starts_with('/') {
            Some(p.to_string())
        } else if p == "models/my.lib" {
            Some(format!("/board/{p}"))
        } else {
            None
        }
    }

    #[test]
    fn kicad_clis_netlist_is_cut_into_head_directives_and_items() {
        let p = split_netlist(CLI, &directives());
        assert_eq!(p.head.len(), 3);
        assert_eq!(p.directives, directives());
        assert_eq!(p.items.len(), 3);
        // without the directive texts the items follow the head
        let q = split_netlist(".title KiCad schematic\nC1 a GND 1u\n+ ic=0\nR1 a b 1k\n.end\n", &[]);
        assert_eq!(q.items, vec!["C1 a GND 1u ic=0", "R1 a b 1k"]);
        // in any order, kept in the netlist's; one not found leaves its dot lines as directives
        assert_eq!(split_netlist(CLI, &[".tran 1u 1m".into(), ".save v(out)".into()]).directives, directives());
        let r = split_netlist(CLI, &[".tran ${T}".into()]);
        assert_eq!(r.directives, directives());
        assert_eq!(r.items.len(), 3);
    }

    #[test]
    fn the_tab_command_replaces_the_schematics_analysis() {
        let n = simulator_netlist(CLI, &directives(), ".ac dec 10 1 1Meg", OPTION_DEFAULT_FLAGS, &BTreeMap::new(), &resolve);
        let want = ".title KiCad schematic\n.include \"/usr/share/kicad/symbols/Simulation_SPICE.sp\"\n.include \"/board/models/my.lib\"\n.save all\n.probe alli\n.probe p(XU1)\n.probe p(R2)\n.probe p(V1)\n.save v(out)\n.ac dec 10 1 1Meg\nXU1 GND /INV VCC /VEE /OUT kicad_builtin_opamp\nR2 /INV /OUT 100k\nV1 /IN GND DC 0 SIN( 0 1 1k 0 0 0 ) AC 1\n.end\n";
        assert_eq!(n.text, want);
        assert!(n.messages.is_empty());
        // an empty command keeps the schematic's analyses; no option adds nothing and leaves the paths alone
        let m = simulator_netlist(CLI, &directives(), "", 0, &BTreeMap::new(), &resolve);
        assert!(m.text.contains(".save v(out)\n.tran 1u 1m\nXU1"), "{}", m.text);
        assert!(m.text.contains(".include \"models/my.lib\"") && !m.text.contains(".save all"));
    }

    #[test]
    fn a_library_that_is_not_found_is_reported() {
        let n = simulator_netlist(".title KiCad schematic\n.include \"nowhere.lib\"\nR1 a 0 1k\n.end\n", &[], ".op", OPTION_ADJUST_INCLUDE_PATHS, &BTreeMap::new(), &resolve);
        assert_eq!(n.messages, vec!["Could not find library file 'nowhere.lib'"]);
        assert!(n.text.contains(".include \"nowhere.lib\"\n.op\nR1"));
    }

    #[test]
    fn a_netlist_without_items_gets_no_directives() {
        let n = simulator_netlist(".title KiCad schematic\n.end\n", &[], ".op", OPTION_DEFAULT_FLAGS, &BTreeMap::new(), &resolve);
        assert_eq!(n.text, ".title KiCad schematic\n.end\n");
    }

    #[test]
    fn spice_net_names_follow_kicads_markup_conversion() {
        assert_eq!(spice_net_name("Net-(C1-Pad1)"), "Net-_C1-Pad1_");
        assert_eq!(spice_net_name("/V_{OUT}"), "/VOUT");
        assert_eq!(spice_net_name("~{RESET}"), "_RESET");
        assert_eq!(spice_net_name("//foo"), "/root/foo");
        assert_eq!(spice_net_name("unconnected-(V3-Pad1)"), "unconnected-_V3-Pad1_");
    }

    fn comp(r: &str, pins: &[&str], fields: &[(&str, &str)]) -> Comp {
        Comp { reference: r.into(), value: String::new(), pins: pins.iter().map(|s| s.to_string()).collect(), fields: fields.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(), lib_id: String::new() }
    }

    #[test]
    fn the_circuit_knows_names_pins_currents_and_tuners() {
        let cli = ".title KiCad schematic\nXU1 GND /INV VCC /VEE /OUT kicad_builtin_opamp\nR2 /INV /OUT 100k\nV1 /IN GND DC 5\nV2 a GND DC 0 SIN( 0 1 1k 0 0 0 ) AC 1\nQ1 c b e QNPN\n.end\n";
        let xml = XmlNetlist {
            comps: vec![
                comp("U1", &["1", "2", "3", "4", "5"], &[("Sim.Pins", "1=in+ 2=in- 3=vcc 4=vee 5=out"), ("Sim.Device", "SUBCKT")]),
                comp("R2", &["2", "1"], &[]),
                comp("V1", &["1", "2"], &[("Sim.Pins", "1=+ 2=-")]),
                comp("V2", &["1", "2"], &[("Sim.Pins", "1=+ 2=-")]),
                comp("Q1", &["1", "2", "3"], &[("Sim.Pins", "1=B 2=C 3=E")]),
            ],
            nets: vec![XmlNet { name: "/IN".into(), nodes: vec![] }],
        };
        let ports = subckt_ports(".subckt kicad_builtin_opamp in+ in- vcc vee out params: POLE=20 GAIN=20k\n.ends\n");
        let items = circuit(&split_netlist(cli, &[]), &xml, &ports);
        let u = &items[0];
        assert_eq!((u.reference.as_str(), u.name.as_str(), u.device, u.pin_count), ("U1", "XU1", 'X', 5));
        assert_eq!(u.currents[4], "I(XU1:out)");
        assert_eq!(u.pin_index["5"], 4);
        assert_eq!(u.nodes[4], "/OUT");
        let r = &items[1];
        assert_eq!(r.currents, vec!["I(R2)"]);
        assert_eq!(r.tuner, Some(("r".into(), "100k".into())));
        assert_eq!(r.pin_index["1"], 0);
        assert_eq!(items[2].tuner, Some(("dc".into(), "5".into())));
        assert_eq!(items[3].tuner, None, "a SIN source is not tuned");
        let q = &items[4];
        assert_eq!(q.currents, vec!["I(Q1:c)", "I(Q1:b)", "I(Q1:e)"]);
        assert_eq!(q.pin_index["1"], 1, "pin 1 is the base");
        assert_eq!(circuit_nets(&items[..2]), vec!["/INV", "/OUT", "/VEE", "GND", "VCC"]);
    }
}
