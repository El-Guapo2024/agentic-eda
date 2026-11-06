// Bus notation in the patterns that assign nets to net classes: `D[0..7]` and `{SDA SCL}` stand for their members. KiCad expands a pattern into one pattern a
// member whenever one is set (`NET_SETTINGS::SetNetclassPatternAssignment`: the Net Classes page's OK, Assign Netclass's OK) and when the project's patterns are
// read, "because the regex/wildcard matchers interpret brackets and braces as special characters, not as bus notation". The page expands its rows the same way
// before it counts the nets of each class and before Apply; the backend does it again (`eda_model::bus_pattern`, the same port, both held to
// src/kicad/bus_patterns.json).
//
// Ported from common/project/net_settings.cpp of KiCad 8303b2ad -- `NET_SETTINGS::ParseBusVector`, `ParseBusGroup`, `ForEachBusMember`, their helpers
// `isSuperSubOverbar`, `isEscaped` and `escapeSpacesForBus` -- and common/string_utils.cpp `EscapeString( .., CTX_NETNAME )` for a group's members, character
// for character, quirks included (a range with no start or end reads it as 0; `A{2}` reads as a group, the pattern `2`; a member `x/y`, written `x{slash}y`,
// reads again as a group of one, `slash`).

/** `isSuperSubOverbar`. */
function isSuperSubOverbar(c: string | undefined): boolean {
  return c === "_" || c === "^" || c === "~";
}

/** `isEscaped`: an odd run of backslashes before `pos`. */
function isEscaped(s: readonly string[], pos: number): boolean {
  let n = 0;
  for (let p = pos; p > 0 && s[p - 1] === "\\"; p--) n++;
  return n % 2 === 1;
}

/** `EscapeString( .., CTX_NETNAME )`: `/` written `{slash}`, line breaks dropped. */
function escapeNetname(s: string): string {
  let out = "";
  for (const c of s) out += c === "/" ? "{slash}" : c === "\n" || c === "\r" ? "" : c;
  return out;
}

const isDigit = (c: string | undefined) => c !== undefined && c >= "0" && c <= "9";

/** `NET_SETTINGS::ParseBusVector`: the members of a vector bus `PREFIX[begin..end]SUFFIX`, or null when the text is not one. */
export function parseBusVector(bus: string): string[] | null {
  const b = Array.from(bus);
  const len = b.length;
  let i = 0;
  const prefix: string[] = [];
  let suffix = "";
  let tmp = "";
  let begin = 0;
  let end = 0;
  let braceNesting = 0;
  let fmtWrapsName = false;
  let inQuotes = false;

  // Parse prefix
  while (i < len) {
    const c = b[i]!;
    if (c === '"' && !isEscaped(b, i)) {
      inQuotes = !inQuotes;
      i++;
      continue;
    }
    if (inQuotes) {
      if (c === "\\" && i + 1 < len) prefix.push(b[++i]!);
      else prefix.push(c);
      i++;
      continue;
    }
    if (c === "{") {
      if (i > 0 && isSuperSubOverbar(b[i - 1])) {
        braceNesting++;
        prefix.push("{");
        i++;
        continue;
      }
      return null;
    } else if (c === "}") {
      braceNesting--;
      prefix.push("}");
      i++;
      continue;
    }
    if (c === "\\" && i + 1 < len && b[i + 1] === " ") {
      prefix.push(b[++i]!);
      i++;
      continue;
    }
    if (c === " " || c === "]") return null;
    if (c === "[") {
      if (braceNesting > 0) {
        const fmtStart = prefix.lastIndexOf("{");
        if (fmtStart > 0 && isSuperSubOverbar(prefix[fmtStart - 1])) {
          // '{' immediately precedes '[' (D_{[1..2]}): the formatting decorates the range; name characters between them (~{BE[0..3]}): it wraps the name.
          if (fmtStart === prefix.length - 1) prefix.length = fmtStart - 1;
          else fmtWrapsName = true;
        }
      }
      break;
    }
    prefix.push(c);
    i++;
  }

  // Parse start number
  i++; // '[' character
  if (i >= len) return null;
  while (i < len) {
    if (b[i] === "." && i + 1 < len && b[i + 1] === ".") {
      if (tmp !== "") begin = Number(tmp);
      i += 2;
      break;
    }
    if (!isDigit(b[i])) return null;
    tmp += b[i];
    i++;
  }

  // Parse end number
  tmp = "";
  if (i >= len) return null;
  while (i < len) {
    if (b[i] === "]") {
      if (tmp !== "") end = Number(tmp);
      i++;
      break;
    }
    if (!isDigit(b[i])) return null;
    tmp += b[i];
    i++;
  }

  // Parse suffix
  for (; i < len; i++) {
    const c = b[i]!;
    if (c === "}") {
      braceNesting--;
      if (fmtWrapsName) suffix += c;
    } else if (c === "+" || c === "-" || c === "P" || c === "N") suffix += c;
    else return null;
  }

  if (braceNesting !== 0 || begin === end) return null;
  if (begin > end) [begin, end] = [end, begin];
  const name = prefix.join("");
  const out: string[] = [];
  for (let n = begin; n <= end; n++) out.push(`${name}${n}${suffix}`);
  return out;
}

/** `NET_SETTINGS::ParseBusGroup`: the members of a bus group `PREFIX{member member ..}` (each `EscapeString( escapeSpacesForBus( m ), CTX_NETNAME )`), or null. */
export function parseBusGroup(group: string): string[] | null {
  const g = Array.from(group);
  const len = g.length;
  let i = 0;
  let tmp = "";
  let braceNesting = 0;
  let inQuotes = false;
  const members: string[] = [];
  const push = (m: string) => {
    if (m !== "") members.push(escapeNetname(m.split(" ").join("\\ ")));
  };

  // Parse prefix
  while (i < len) {
    const c = g[i]!;
    if (c === '"' && !isEscaped(g, i)) {
      inQuotes = !inQuotes;
      i++;
      continue;
    }
    if (inQuotes) {
      if (c === "\\" && i + 1 < len) i++;
      i++;
      continue;
    }
    if (c === "{") {
      if (i > 0 && isSuperSubOverbar(g[i - 1])) {
        braceNesting++;
        i++;
        continue;
      }
      break;
    } else if (c === "}") {
      braceNesting--;
      i++;
      continue;
    }
    if (c === "\\" && i + 1 < len && g[i + 1] === " ") {
      i += 2;
      continue;
    }
    if (c === " " || c === "[" || c === "]") return null;
    i++;
  }
  if (braceNesting !== 0) return null;

  // Parse members
  i++; // '{' character
  if (i >= len) return null;
  inQuotes = false;
  while (i < len) {
    const c = g[i]!;
    if (c === '"' && !isEscaped(g, i)) {
      inQuotes = !inQuotes;
      i++;
      continue;
    }
    if (inQuotes) {
      if (c === "\\" && i + 1 < len) tmp += g[++i];
      else tmp += c;
      i++;
      continue;
    }
    if (c === "{") {
      if (i > 0 && isSuperSubOverbar(g[i - 1])) {
        braceNesting++;
        tmp += "{";
        i++;
        continue;
      }
      return null;
    } else if (c === "}") {
      if (braceNesting > 0) {
        braceNesting--;
        tmp += "}";
        i++;
        continue;
      }
      push(tmp);
      return members;
    }
    if (c === "\\" && i + 1 < len && g[i + 1] === " ") {
      tmp += g[++i];
      i++;
      continue;
    }
    if (c === " " || c === ",") {
      push(tmp);
      tmp = "";
      i++;
      continue;
    }
    tmp += c;
    i++;
  }
  return null;
}

/** `NET_SETTINGS::ForEachBusMember`: a vector's members, each member of a group expanded again, or the pattern itself. */
export function busMembers(pattern: string, depth = 0): string[] {
  const vector = parseBusVector(pattern);
  if (vector) return vector;
  const group = depth < 64 ? parseBusGroup(pattern) : null;
  if (group) return group.flatMap((m) => busMembers(m, depth + 1));
  return [pattern];
}

/** The patterns `SetNetclassPatternAssignment` keeps for `patterns`, in order: each one's bus members, an exact repeat once (`addSinglePatternAssignment`). */
export function expandPatterns(patterns: readonly string[]): string[] {
  const out: string[] = [];
  for (const p of patterns) for (const m of busMembers(p)) if (!out.includes(m)) out.push(m);
  return out;
}
