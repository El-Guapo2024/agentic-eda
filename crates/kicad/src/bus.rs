//! Bus connectivity (GAPS.md #20): vector (`NAME[0..7]`) and group
//! (`{A B}`/`NAME{A B}`) bus-name expansion, project-scoped bus aliases,
//! and the two within-one-sheet ERC checks raw geometry alone can decide
//! (`bus_to_net_conflict`, `net_not_bus_member`). The hierarchy-*boundary*
//! bus check (`bus_to_bus_conflict`) and the per-member cross-sheet union
//! live in `crate::hierarchy`/`crate::erc::check_hierarchy` instead, right
//! next to the scalar hierarchical-label join/check they each generalize --
//! see those modules' own docs.
//!
//! Ported from `NET_SETTINGS::ParseBusVector`/`ParseBusGroup`
//! (`common/project/net_settings.cpp`), `SCH_CONNECTION::ConfigureFromLabel`
//! (`sch_connection.cpp`, for the alias-resolution and named-group-prefix
//! rules), and `CONNECTION_GRAPH::ercCheckBusToNetConflicts`/
//! `ercCheckBusToBusEntryConflicts` (`connection_graph.cpp`).
//!
//! Three KiCad behaviors are deliberately not ported, each documented at
//! its own call site below: the `^{}`/`_{}`/`~{}` super/sub/overbar
//! text-formatting markers a vector/group prefix can carry (purely
//! cosmetic text rendering, orthogonal to bus electrical semantics, and
//! this project has no text-formatting notation anywhere else either);
//! quoted or backslash-escaped group member names containing literal
//! spaces (members here are whitespace-separated only); and
//! `SCH_BUS_BUS_ENTRY` (bus-to-bus entries) -- real KiCad's own file
//! writer never emits one any more (silently downgrades to a plain bus
//! line on save) and no current UI action creates one, so there is
//! nothing to round-trip.
//!
//! `ERCE_DRIVER_CONFLICT` (`multiple_net_names` in this project's naming --
//! two differently-named drivers resolving to one net) is a real, separate
//! KiCad check that happens to run in the same per-subgraph ERC loop
//! upstream, but it is not bus-specific (it fires for an ordinary net
//! too) and would need `sch_import::reconcile` to expose the *candidate
//! name set* behind each resolved net rather than just its one winning
//! name. Left out of this port as a documented scope cut, not folded in
//! here just because upstream happens to test it alongside these.

use std::collections::BTreeMap;

use eda_model::ir::{BusAlias, Point, SchematicSection};
use eda_model::CheckResult;

const MAX_BUS_RECURSION: u32 = 8;

/// Expands a bus-shaped name (vector or group syntax) to its member net
/// names; `None` when `name` is not bus-shaped at all -- an ordinary net
/// name, *including* a bare alias name used with no surrounding braces.
/// Real KiCad only ever resolves an alias as a group *member* token
/// (`{MY_ALIAS}`, or nested inside another alias's own member list) --
/// `SCH_CONNECTION::IsBusLabel`/`ConfigureFromLabel` call only
/// `ParseBusVector`/`ParseBusGroup` on a label's own raw text, never a bare
/// alias lookup, so a wire or label literally named just `"USB"` (no
/// braces), even with a `BusAlias { name: "USB", .. }` declared, is an
/// ordinary single net in real KiCad too.
pub fn expand_bus_members(name: &str, aliases: &[BusAlias]) -> Option<Vec<String>> {
    expand_inner(name, aliases, 0, false)
}

pub fn is_bus_name(name: &str, aliases: &[BusAlias]) -> bool {
    expand_bus_members(name, aliases).is_some()
}

/// `allow_bare_alias`: true only when expanding one `{...}`-group member
/// token (or one alias's own member entry) -- never at the top-level
/// public call, matching the real asymmetry documented on
/// [`expand_bus_members`].
fn expand_inner(name: &str, aliases: &[BusAlias], depth: u32, allow_bare_alias: bool) -> Option<Vec<String>> {
    if depth > MAX_BUS_RECURSION {
        return None;
    }
    if allow_bare_alias {
        if let Some(alias) = aliases.iter().find(|a| a.name == name) {
            // Each of the alias's own members is itself fully re-expanded
            // (`ConfigureFromLabel`'s own recursive
            // `member->ConfigureFromLabel(alias_member)` call): almost
            // always a literal leaf net name, but it can itself be a
            // vector/group/another alias reference.
            return Some(alias.members.iter().flat_map(|m| expand_inner(m, aliases, depth + 1, true).unwrap_or_else(|| vec![m.clone()])).collect());
        }
    }
    if let Some(members) = parse_vector(name) {
        return Some(members);
    }
    parse_group(name, aliases, depth)
}

/// A bus vector ("DATA[0..7]", "A[7..0]", a differential-pair suffix
/// "D[0..1]+"/"-"/"P"/"N"): prefix, `[`, an unsigned start index, `..`, an
/// unsigned end index, `]`, then an optional one-character-repeated
/// `+`/`-`/`P`/`N` suffix. Members are always emitted in ascending index
/// order regardless of which of start/end was larger in the text (KiCad
/// swaps); equal start/end is rejected outright (a vector needs at least
/// two members) -- both exactly match `NET_SETTINGS::ParseBusVector`.
fn parse_vector(name: &str) -> Option<Vec<String>> {
    let open = name.find('[')?;
    let prefix = &name[..open];
    if prefix.is_empty() || prefix.contains(' ') || prefix.contains('{') || prefix.contains('}') {
        return None;
    }
    let rest = &name[open + 1..];
    let close = rest.find(']')?;
    let range = &rest[..close];
    let suffix = &rest[close + 1..];
    if !suffix.chars().all(|c| matches!(c, '+' | '-' | 'P' | 'N')) {
        return None;
    }
    let (lo_str, hi_str) = range.split_once("..")?;
    if lo_str.is_empty() || hi_str.is_empty() || !lo_str.bytes().all(|b| b.is_ascii_digit()) || !hi_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let lo: i64 = lo_str.parse().ok()?;
    let hi: i64 = hi_str.parse().ok()?;
    if lo == hi {
        return None;
    }
    let (lo, hi) = (lo.min(hi), lo.max(hi));
    Some((lo..=hi).map(|i| format!("{prefix}{i}{suffix}")).collect())
}

/// A bus group ("{A B C}", "NAME{A B C}"): an optional prefix before `{`,
/// then whitespace-separated member tokens up to the matching final `}`.
/// A *named* group ("NAME{...}") prefixes every resulting leaf member with
/// `"NAME."`; an unnamed one ("{...}") leaves members bare --
/// `ConfigureFromLabel`'s own "named bus groups generate a net prefix,
/// unnamed ones don't" rule. Each raw member token is itself fully
/// re-expanded (an alias reference first, matching source's own
/// alias-checked-before-recursing order, then a nested vector/group, else
/// a literal leaf net name).
fn parse_group(name: &str, aliases: &[BusAlias], depth: u32) -> Option<Vec<String>> {
    let open = name.find('{')?;
    if !name.ends_with('}') || name.len() < open + 2 {
        return None;
    }
    let prefix = &name[..open];
    if prefix.contains(' ') || prefix.contains('[') {
        return None;
    }
    let inner = &name[open + 1..name.len() - 1];
    let tokens: Vec<&str> = inner.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }
    let members: Vec<String> = tokens.into_iter().flat_map(|tok| expand_inner(tok, aliases, depth + 1, true).unwrap_or_else(|| vec![tok.to_string()])).collect();
    Some(if prefix.is_empty() { members } else { members.into_iter().map(|m| format!("{prefix}.{m}")).collect() })
}

// ---------------------------------------------------------------- within-sheet ERC

/// Runs both within-sheet bus checks once per screen (GAPS.md #20) --
/// screen-scoped the same way `check_hierarchy`'s `duplicate_sheet_names`
/// is (`crate::erc::every_screen`: once per unique screen, not once per
/// placement of a screen instanced more than once), since bus geometry is
/// entirely local to one sheet's own canvas. Call with the *original*,
/// unflattened design -- see `crate::erc::check_erc`'s own
/// `original_design` binding for why (a flattened mega-sheet's wires are
/// every visited sheet's geometry dumped into one list, which would make
/// a bus entry match the wrong sheet's bus run).
pub fn check_bus(design: &eda_model::ir::Design, out: &mut Vec<CheckResult>) {
    let mut ok_net_conflict = true;
    let mut ok_entry = true;
    for (_, sch) in crate::erc::every_screen(design) {
        check_bus_to_net_conflicts(sch, out, &mut ok_net_conflict);
        check_bus_entry_conflicts(sch, &design.bus_aliases, out, &mut ok_entry);
    }
    if ok_net_conflict {
        out.push(CheckResult::pass("bus_to_net_conflict"));
    }
    if ok_entry {
        out.push(CheckResult::pass("net_not_bus_member"));
    }
}

/// (ERCE_BUS_TO_NET_CONFLICT) ported from
/// `CONNECTION_GRAPH::ercCheckBusToNetConflicts`: a plain net wire/label
/// touching a bus wire/label directly, with no [`eda_model::ir::BusEntry`]
/// between them, resolves both onto the *same* reconciled point-group --
/// grouping `sch.wires` by their already-reconciled `.net` (every wire
/// `sch_import::reconcile` merged into one point-touching group ends up
/// with that one shared name), a group counts as "bus" when any of its own
/// wires carries `bus: true` or any label sharing its name is itself
/// bus-shaped, and "net" symmetrically -- a group that is *both* is the
/// conflict. Checking the *resolved group's own flags* (not just whether
/// its final name happens to parse as bus-shaped) is what keeps an
/// unlabeled, all-bus-wire run with a synthesized `"NET_7"`-style
/// fallback name (`reconcile`'s own anonymous-group convention) from
/// false-firing this check against itself.
fn check_bus_to_net_conflicts(sch: &SchematicSection, out: &mut Vec<CheckResult>, ok: &mut bool) {
    let mut wires_by_net: BTreeMap<&str, Vec<&eda_model::ir::Wire>> = BTreeMap::new();
    for w in &sch.wires {
        wires_by_net.entry(w.net.as_str()).or_default().push(w);
    }
    for (name, wires) in wires_by_net {
        let any_bus_wire = wires.iter().any(|w| w.bus);
        let any_net_wire = wires.iter().any(|w| !w.bus);
        let label_here = sch.labels.iter().any(|l| l.net == name);
        let name_is_bus = is_bus_name(name, &[]);
        let is_bus_group = any_bus_wire || (label_here && name_is_bus);
        let is_net_group = any_net_wire || (label_here && !name_is_bus);
        if is_bus_group && is_net_group {
            let at = wires.first().and_then(|w| w.pts.first()).copied().unwrap_or(Point { x: 0, y: 0 });
            out.push(CheckResult::fail("bus_to_net_conflict", format!("{},{}", at.x, at.y), format!("a plain net and bus '{name}' are connected directly, with no bus entry between them")));
            *ok = false;
        }
    }
}

/// (ERCE_BUS_ENTRY_CONFLICT, this project's `net_not_bus_member`) ported
/// from `CONNECTION_GRAPH::ercCheckBusToBusEntryConflicts`: for each
/// [`eda_model::ir::BusEntry`], find which of its two endpoints
/// (`at`, `at + size`) lands on a `bus: true` wire (the "bus side" --
/// `None` when neither or both do, which is not this check's concern:
/// either the entry is dangling/mis-touching something else entirely, or
/// it sits mid-bus with no real net-side wire yet). The bus side's own
/// resolved name, expanded to its member list, must contain whatever name
/// the *other* endpoint resolved to -- a label on the net-side stub is the
/// overwhelming common real pattern, and already gives that endpoint its
/// real member name through nothing more than `reconcile`'s existing
/// label-name mechanism, so this check needs no bus-specific connectivity
/// of its own, only this one membership test.
fn check_bus_entry_conflicts(sch: &SchematicSection, aliases: &[BusAlias], out: &mut Vec<CheckResult>, ok: &mut bool) {
    for entry in &sch.bus_entries {
        let other = Point { x: entry.at.x + entry.size.x, y: entry.at.y + entry.size.y };
        let Some((bus_point, net_point)) = pick_bus_side(sch, entry.at, other) else { continue };
        let Some(bus_name) = crate::erc::net_at_point(&sch.wires, &sch.labels, &sch.power_symbols, bus_point) else { continue };
        let Some(members) = expand_bus_members(&bus_name, aliases) else { continue };
        let net_name = crate::erc::net_at_point(&sch.wires, &sch.labels, &sch.power_symbols, net_point).unwrap_or_default();
        if net_name.is_empty() || !members.contains(&net_name) {
            let label = if net_name.is_empty() { "an unlabeled net".to_string() } else { format!("net '{net_name}'") };
            out.push(CheckResult::fail("net_not_bus_member", format!("{},{}", entry.at.x, entry.at.y), format!("{label} is graphically connected to bus '{bus_name}' but is not a member of it")));
            *ok = false;
        }
    }
}

/// Which of `a`/`b` (a [`eda_model::ir::BusEntry`]'s two endpoints) lands
/// exactly on a `bus: true` wire's own point -- `Some((bus_side, other))`
/// when exactly one does, `None` when neither or both do.
fn pick_bus_side(sch: &SchematicSection, a: Point, b: Point) -> Option<(Point, Point)> {
    let is_bus_point = |p: Point| sch.wires.iter().any(|w| w.bus && w.pts.contains(&p));
    match (is_bus_point(a), is_bus_point(b)) {
        (true, false) => Some((a, b)),
        (false, true) => Some((b, a)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_expands_ascending_regardless_of_text_order() {
        assert_eq!(expand_bus_members("DATA[0..3]", &[]), Some(vec!["DATA0".into(), "DATA1".into(), "DATA2".into(), "DATA3".into()]));
        assert_eq!(expand_bus_members("DATA[3..0]", &[]), Some(vec!["DATA0".into(), "DATA1".into(), "DATA2".into(), "DATA3".into()]), "descending text order still yields ascending members");
    }

    #[test]
    fn vector_rejects_equal_start_and_end() {
        assert_eq!(expand_bus_members("A[2..2]", &[]), None);
    }

    #[test]
    fn vector_keeps_a_differential_pair_suffix() {
        assert_eq!(expand_bus_members("D[0..1]+", &[]), Some(vec!["D0+".into(), "D1+".into()]));
    }

    #[test]
    fn unnamed_group_members_are_bare() {
        assert_eq!(expand_bus_members("{SCL SDA}", &[]), Some(vec!["SCL".into(), "SDA".into()]));
    }

    #[test]
    fn named_group_prefixes_every_member() {
        assert_eq!(expand_bus_members("I2C{SCL SDA}", &[]), Some(vec!["I2C.SCL".into(), "I2C.SDA".into()]));
    }

    #[test]
    fn group_member_that_is_itself_a_vector_expands_recursively() {
        assert_eq!(expand_bus_members("{A[0..1] B}", &[]), Some(vec!["A0".into(), "A1".into(), "B".into()]));
    }

    #[test]
    fn bare_alias_name_with_no_braces_is_not_a_bus() {
        let aliases = [BusAlias { name: "USB".into(), members: vec!["D+".into(), "D-".into()] }];
        assert_eq!(expand_bus_members("USB", &aliases), None, "real KiCad only resolves an alias inside {{...}}, never as a label's own bare text");
    }

    #[test]
    fn alias_referenced_inside_a_group_expands_to_its_members() {
        let aliases = [BusAlias { name: "USB".into(), members: vec!["D+".into(), "D-".into()] }];
        assert_eq!(expand_bus_members("{USB}", &aliases), Some(vec!["D+".into(), "D-".into()]));
    }

    #[test]
    fn not_bus_shaped_at_all_is_a_plain_net() {
        assert_eq!(expand_bus_members("GND", &[]), None);
        assert!(!is_bus_name("GND", &[]));
    }

    fn wire(net: &str, bus: bool, pts: Vec<Point>) -> eda_model::ir::Wire {
        eda_model::ir::Wire { id: String::new(), net: net.into(), pins: vec![], pts, bus }
    }
    fn label(net: &str, at: Point) -> eda_model::ir::NetLabel {
        eda_model::ir::NetLabel { id: String::new(), net: net.into(), at, kind: eda_model::ir::LabelKind::Local }
    }
    fn empty_sch() -> SchematicSection {
        SchematicSection { symbols: vec![], wires: vec![], labels: vec![], texts: vec![], power_symbols: vec![], no_connects: vec![], bus_entries: vec![], erc_exclusions: vec![], erc_pin_map: None, user_fields: Default::default(), title_block: None, sheets: vec![], instance_overrides: vec![], imported_from_kicad: false }
    }

    /// A plain wire whose endpoint coincides with a bus wire's endpoint,
    /// with no entry between them, is exactly `bus_to_net_conflict`. Both
    /// wires already carry the *same* resolved net name here, same as they
    /// would after `sch_import::reconcile` actually merged their touching
    /// point into one group -- this function reads that already-settled
    /// state, it does not merge points itself.
    #[test]
    fn plain_wire_touching_a_bus_wire_directly_is_a_conflict() {
        let junction = Point { x: 10_000, y: 10_000 };
        let sch = SchematicSection {
            wires: vec![wire("DATA[0..3]", true, vec![Point { x: 0, y: 10_000 }, junction]), wire("DATA[0..3]", false, vec![junction, Point { x: 20_000, y: 10_000 }])],
            ..empty_sch()
        };
        let mut out = Vec::new();
        let mut ok = true;
        check_bus_to_net_conflicts(&sch, &mut out, &mut ok);
        assert!(!ok);
        assert!(out.iter().any(|r| r.check == "bus_to_net_conflict"), "{out:#?}");
    }

    /// An unlabeled, fully-bus-flagged run (`reconcile`'s own synthesized
    /// "NET_n" fallback name, since nothing named it) must NOT self-fire --
    /// the regression this function's own doc warns about.
    #[test]
    fn an_unlabeled_all_bus_run_does_not_false_fire() {
        let sch = SchematicSection { wires: vec![wire("NET_1", true, vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }])], ..empty_sch() };
        let mut out = Vec::new();
        let mut ok = true;
        check_bus_to_net_conflicts(&sch, &mut out, &mut ok);
        assert!(ok, "{out:#?}");
    }

    /// The common, correct pattern -- a bus entry's net-side stub labeled
    /// with one of the bus's own real members -- passes clean.
    #[test]
    fn bus_entry_tapping_a_real_member_passes() {
        let bus_pt = Point { x: 10_000, y: 10_000 };
        let net_pt = Point { x: 12_540, y: 12_540 };
        let sch = SchematicSection {
            wires: vec![wire("DATA[0..3]", true, vec![Point { x: 0, y: 10_000 }, bus_pt])],
            bus_entries: vec![eda_model::ir::BusEntry { id: String::new(), at: bus_pt, size: Point { x: 2_540, y: 2_540 } }],
            labels: vec![label("DATA2", net_pt)],
            ..empty_sch()
        };
        let mut out = Vec::new();
        let mut ok = true;
        check_bus_entry_conflicts(&sch, &[], &mut out, &mut ok);
        assert!(ok, "{out:#?}");
    }

    /// A bus entry whose net-side label names something that is *not* one
    /// of the bus's own members is exactly `net_not_bus_member`.
    #[test]
    fn bus_entry_tapping_a_name_outside_the_bus_fails() {
        let bus_pt = Point { x: 10_000, y: 10_000 };
        let net_pt = Point { x: 12_540, y: 12_540 };
        let sch = SchematicSection {
            wires: vec![wire("DATA[0..3]", true, vec![Point { x: 0, y: 10_000 }, bus_pt])],
            bus_entries: vec![eda_model::ir::BusEntry { id: String::new(), at: bus_pt, size: Point { x: 2_540, y: 2_540 } }],
            labels: vec![label("RESET", net_pt)],
            ..empty_sch()
        };
        let mut out = Vec::new();
        let mut ok = true;
        check_bus_entry_conflicts(&sch, &[], &mut out, &mut ok);
        assert!(!ok);
        assert!(out.iter().any(|r| r.check == "net_not_bus_member"), "{out:#?}");
    }
}
