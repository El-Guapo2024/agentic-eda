//! Bus notation in the patterns that assign nets to net classes: `D[0..7]` and `{SDA SCL}` stand for their members (Board Setup > Net Classes,
//! "Netclass Assignments"; Assign Netclass...).
//!
//! KiCad expands a pattern into one pattern a member whenever a pattern is set (`NET_SETTINGS::SetNetclassPatternAssignment`, the page's OK and Assign
//! Netclass's) and when the project file's `netclass_patterns` are read: "the regex/wildcard matchers interpret brackets and braces as special characters, not as
//! bus notation". So `D[0..7]` stored is `D0` .. `D7`, eight patterns of the class, and the page shows eight rows when it opens again. A pattern that is no bus
//! (`USB_D[PM]`, `V*`, `(CLK|DAT)[0-3]`) is itself.
//!
//! Ported from `common/project/net_settings.cpp` of KiCad 8303b2ad -- `NET_SETTINGS::ParseBusVector`, `ParseBusGroup` and `ForEachBusMember` with their helpers
//! `isSuperSubOverbar`, `isEscaped` and the group's `escapeSpacesForBus`, and `common/string_utils.cpp::EscapeString( .., CTX_NETNAME )` for a group's members --
//! character for character, quirks included: a range with no start or no end reads it as 0 (`tmp.ToLong( &begin )` leaves it), a regular expression with a
//! `{n}` count reads as a group (`A{2}` is the pattern `2`), a quoted or escaped space in a group member stays escaped (`my\ net`), and a member with a `/`,
//! written `{slash}`, reads again as a group of one (`x/y` is the pattern `slash`). Not the schematic's
//! buses (`crates/kicad/src/bus.rs`), which prefix a named group's members with the group's name.

/// `isSuperSubOverbar`.
fn is_super_sub_overbar(c: char) -> bool {
    matches!(c, '_' | '^' | '~')
}

/// `isEscaped`: an odd run of backslashes before `pos`.
fn is_escaped(s: &[char], pos: usize) -> bool {
    let mut n = 0;
    let mut p = pos;
    while p > 0 && s[p - 1] == '\\' {
        n += 1;
        p -= 1;
    }
    n % 2 == 1
}

/// `EscapeString( .., CTX_NETNAME )`: `/` written `{slash}`, line breaks dropped.
fn escape_netname(s: &str) -> String {
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

/// `wxString::ToLong` of the digits read: their value, or `None` for no digits (the caller keeps what it had, 0).
fn to_long(digits: &str) -> Option<i64> {
    digits.parse().ok()
}

/// `NET_SETTINGS::ParseBusVector`: a vector bus `PREFIX[begin..end]SUFFIX` and its members `PREFIX<n>SUFFIX` for n from the smaller to the larger end, or
/// `None` when the text is not one.
pub fn parse_bus_vector(bus: &str) -> Option<Vec<String>> {
    let b: Vec<char> = bus.chars().collect();
    let len = b.len();
    let mut i = 0;
    let mut prefix: Vec<char> = Vec::new();
    let mut suffix = String::new();
    let mut tmp = String::new();
    let mut begin: i64 = 0;
    let mut end: i64 = 0;
    let mut brace_nesting: i32 = 0;
    let mut fmt_wraps_name = false;
    let mut in_quotes = false;

    // Parse prefix
    while i < len {
        let c = b[i];
        if c == '"' && !is_escaped(&b, i) {
            in_quotes = !in_quotes;
            i += 1;
            continue;
        }
        if in_quotes {
            if c == '\\' && i + 1 < len {
                i += 1;
                prefix.push(b[i]);
            } else {
                prefix.push(c);
            }
            i += 1;
            continue;
        }
        if c == '{' {
            if i > 0 && is_super_sub_overbar(b[i - 1]) {
                brace_nesting += 1;
                prefix.push('{');
                i += 1;
                continue;
            }
            return None;
        } else if c == '}' {
            brace_nesting -= 1;
            prefix.push('}');
            i += 1;
            continue;
        }
        if c == '\\' && i + 1 < len && b[i + 1] == ' ' {
            i += 1;
            prefix.push(b[i]);
            i += 1;
            continue;
        }
        if c == ' ' || c == ']' {
            return None;
        }
        if c == '[' {
            if brace_nesting > 0 {
                if let Some(fmt_start) = prefix.iter().rposition(|&p| p == '{') {
                    if fmt_start > 0 && is_super_sub_overbar(prefix[fmt_start - 1]) {
                        if fmt_start == prefix.len() - 1 {
                            // '{' immediately precedes '[' (e.g. D_{[1..2]}): the formatting decorates the range indices, not the name itself.
                            prefix.truncate(fmt_start - 1);
                        } else {
                            // Name characters between '{' and '[' (e.g. ~{BE[0..3]}): the formatting wraps the signal name, not the range.
                            fmt_wraps_name = true;
                        }
                    }
                }
            }
            break;
        }
        prefix.push(c);
        i += 1;
    }

    // Parse start number
    i += 1; // '[' character
    if i >= len {
        return None;
    }
    while i < len {
        if b[i] == '.' && i + 1 < len && b[i + 1] == '.' {
            if let Some(v) = to_long(&tmp) {
                begin = v;
            }
            i += 2;
            break;
        }
        if !b[i].is_ascii_digit() {
            return None;
        }
        tmp.push(b[i]);
        i += 1;
    }

    // Parse end number
    tmp.clear();
    if i >= len {
        return None;
    }
    while i < len {
        if b[i] == ']' {
            if let Some(v) = to_long(&tmp) {
                end = v;
            }
            i += 1;
            break;
        }
        if !b[i].is_ascii_digit() {
            return None;
        }
        tmp.push(b[i]);
        i += 1;
    }

    // Parse suffix
    while i < len {
        match b[i] {
            '}' => {
                brace_nesting -= 1;
                if fmt_wraps_name {
                    suffix.push('}');
                }
            }
            c @ ('+' | '-' | 'P' | 'N') => suffix.push(c),
            _ => return None,
        }
        i += 1;
    }

    if brace_nesting != 0 || begin == end {
        return None;
    }
    if begin > end {
        std::mem::swap(&mut begin, &mut end);
    }
    let prefix: String = prefix.into_iter().collect();
    Some((begin..=end).map(|n| format!("{prefix}{n}{suffix}")).collect())
}

/// `NET_SETTINGS::ParseBusGroup`: a bus group `PREFIX{member member ..}` and its members (each written as `EscapeString( escapeSpacesForBus( m ), CTX_NETNAME )`),
/// or `None` when the text is not one.
pub fn parse_bus_group(group: &str) -> Option<Vec<String>> {
    let g: Vec<char> = group.chars().collect();
    let len = g.len();
    let mut i = 0;
    let mut tmp = String::new();
    let mut brace_nesting: i32 = 0;
    let mut in_quotes = false;
    let mut members: Vec<String> = Vec::new();
    // `escapeSpacesForBus`
    let push = |members: &mut Vec<String>, m: &str| {
        if !m.is_empty() {
            members.push(escape_netname(&m.replace(' ', "\\ ")));
        }
    };

    // Parse prefix
    while i < len {
        let c = g[i];
        if c == '"' && !is_escaped(&g, i) {
            in_quotes = !in_quotes;
            i += 1;
            continue;
        }
        if in_quotes {
            if c == '\\' && i + 1 < len {
                i += 1;
            }
            i += 1;
            continue;
        }
        if c == '{' {
            if i > 0 && is_super_sub_overbar(g[i - 1]) {
                brace_nesting += 1;
                i += 1;
                continue;
            }
            break;
        } else if c == '}' {
            brace_nesting -= 1;
            i += 1;
            continue;
        }
        if c == '\\' && i + 1 < len && g[i + 1] == ' ' {
            i += 2;
            continue;
        }
        if c == ' ' || c == '[' || c == ']' {
            return None;
        }
        i += 1;
    }
    if brace_nesting != 0 {
        return None;
    }

    // Parse members
    i += 1; // '{' character
    if i >= len {
        return None;
    }
    in_quotes = false;
    while i < len {
        let c = g[i];
        if c == '"' && !is_escaped(&g, i) {
            in_quotes = !in_quotes;
            i += 1;
            continue;
        }
        if in_quotes {
            if c == '\\' && i + 1 < len {
                i += 1;
                tmp.push(g[i]);
            } else {
                tmp.push(c);
            }
            i += 1;
            continue;
        }
        if c == '{' {
            if i > 0 && is_super_sub_overbar(g[i - 1]) {
                brace_nesting += 1;
                tmp.push('{');
                i += 1;
                continue;
            }
            return None;
        } else if c == '}' {
            if brace_nesting > 0 {
                brace_nesting -= 1;
                tmp.push('}');
                i += 1;
                continue;
            }
            push(&mut members, &tmp);
            return Some(members);
        }
        if c == '\\' && i + 1 < len && g[i + 1] == ' ' {
            i += 1;
            tmp.push(g[i]);
            i += 1;
            continue;
        }
        if c == ' ' || c == ',' {
            push(&mut members, &tmp);
            tmp.clear();
            i += 1;
            continue;
        }
        tmp.push(c);
        i += 1;
    }
    None
}

/// `NET_SETTINGS::ForEachBusMember`: the members of a vector bus, each member of a group expanded again (it may be a vector or a group itself), or the pattern
/// itself when it is neither.
pub fn bus_members(pattern: &str) -> Vec<String> {
    let mut out = Vec::new();
    for_each_bus_member(pattern, &mut |m| out.push(m.to_string()), 0);
    out
}

fn for_each_bus_member(pattern: &str, f: &mut dyn FnMut(&str), depth: usize) {
    if let Some(members) = parse_bus_vector(pattern) {
        for m in &members {
            f(m);
        }
    } else if let Some(members) = parse_bus_group(pattern).filter(|_| depth < 64) {
        for m in &members {
            for_each_bus_member(m, f, depth + 1);
        }
    } else {
        f(pattern);
    }
}

/// The patterns `NET_SETTINGS::SetNetclassPatternAssignment` keeps for `patterns`, in order: each one's bus members, an exact repeat left out
/// (`addSinglePatternAssignment`: "Avoid exact duplicates").
pub fn expand_patterns(patterns: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in patterns {
        for m in bus_members(p) {
            if !out.contains(&m) {
                out.push(m);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cases both ports are held to: what each pattern stands for.
    fn cases() -> Vec<(&'static str, Vec<String>)> {
        let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        vec![
            ("D[0..3]", v(&["D0", "D1", "D2", "D3"])),
            ("A[3..1]", v(&["A1", "A2", "A3"])),
            ("D[0..1]+", v(&["D0+", "D1+"])),
            ("D[0..1]P", v(&["D0P", "D1P"])),
            ("A[2..2]", v(&["A[2..2]"])),
            ("D[..2]", v(&["D0", "D1", "D2"])),
            ("D[1..]", v(&["D0", "D1"])),
            ("I^{2}C[0..1]", v(&["I^{2}C0", "I^{2}C1"])),
            ("D_{[1..2]}", v(&["D1", "D2"])),
            ("~{BE[0..1]}", v(&["~{BE0}", "~{BE1}"])),
            ("{SDA SCL}", v(&["SDA", "SCL"])),
            ("I2C{SDA,SCL}", v(&["SDA", "SCL"])),
            ("{D[0..1] CLK}", v(&["D0", "D1", "CLK"])),
            ("{\"my net\" x/y}", v(&["my\\ net", "slash"])),
            ("{~{CAS} RAS}", v(&["~{CAS}", "RAS"])),
            ("USB_D[PM]", v(&["USB_D[PM]"])),
            ("(CLK|DAT)[0-3]", v(&["(CLK|DAT)[0-3]"])),
            ("V*", v(&["V*"])),
            ("A{2}", v(&["2"])),
            ("D[0..1", v(&["D[0..1"])),
            ("{A", v(&["{A"])),
            ("", v(&[""])),
        ]
    }

    #[test]
    fn bus_patterns_expand_as_net_settings_expands_them() {
        for (pattern, want) in cases() {
            assert_eq!(bus_members(pattern), want, "{pattern}");
        }
        assert_eq!(expand_patterns(&["D[0..1]".into(), "D1".into(), "GND".into()]), vec!["D0", "D1", "GND"], "an exact repeat is kept once");
    }

    /// The Net Classes page expands a pattern the same way before Apply (`web/studio/src/kicad-port/busPattern.ts`); both are held to
    /// `web/studio/src/kicad/bus_patterns.json` (`EDA_WRITE_FIXTURES=1` rewrites it).
    #[test]
    fn the_web_pages_bus_patterns_are_the_ones_expanded_here() {
        let fixture: Vec<serde_json::Value> = cases().into_iter().map(|(p, _)| serde_json::json!({ "pattern": p, "members": bus_members(p) })).collect();
        let fixture = serde_json::Value::Array(fixture);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/studio/src/kicad/bus_patterns.json");
        if std::env::var_os("EDA_WRITE_FIXTURES").is_some() {
            std::fs::write(&path, serde_json::to_string_pretty(&fixture).unwrap() + "\n").unwrap();
        }
        let on_disk: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_default()).unwrap_or(serde_json::Value::Null);
        assert_eq!(on_disk, fixture, "{} is stale: run `EDA_WRITE_FIXTURES=1 cargo test -p eda-model bus_patterns` and commit it", path.display());
    }
}
