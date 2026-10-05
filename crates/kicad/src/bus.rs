//! Bus member names (GAPS.md #20): vector (`NAME[0..7]`) and group
//! (`{A B}`/`NAME{A B}`) bus-name expansion with project-scoped bus aliases.
//! The studio's "Unfold from Bus" menu lists a bus's members with it.
//!
//! This is name parsing, not a rule: the bus ERC checks (`bus_to_net_conflict`,
//! `net_not_bus_member`, `bus_to_bus_conflict`) are kicad-cli's, which reads
//! the same buses from the derived `.kicad_sch`.
//!
//! Ported from `NET_SETTINGS::ParseBusVector`/`ParseBusGroup`
//! (`common/project/net_settings.cpp`) and `SCH_CONNECTION::ConfigureFromLabel`
//! (`sch_connection.cpp`, for the alias-resolution and named-group-prefix
//! rules).
//!
//! Three KiCad behaviors are deliberately not ported: the `^{}`/`_{}`/`~{}`
//! super/sub/overbar text-formatting markers a vector/group prefix can carry
//! (cosmetic text rendering, orthogonal to bus electrical semantics); quoted
//! or backslash-escaped group member names containing literal spaces (members
//! here are whitespace-separated only); and `SCH_BUS_BUS_ENTRY` (real KiCad's
//! own file writer never emits one any more).

use eda_model::ir::BusAlias;

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
    }
}
