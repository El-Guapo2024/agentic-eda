// Label text escaping -- `EscapeString( CTX_NETNAME )` / `UnescapeString` (common/string_utils.cpp) and the
// `getValidNetname` lambda of `SCH_EDIT_TOOL::ChangeTextType`: what a label's text becomes when it is made from
// free text, and what free text a label turns back into.

/** `EscapeString( aSource, CTX_NETNAME )`: `/` becomes `{slash}`, line breaks are dropped. */
export function escapeNetname(s: string): string {
  let out = "";
  for (const c of s) {
    if (c === "/") out += "{slash}";
    else if (c === "\n" || c === "\r") continue;
    else out += c;
  }
  return out;
}

const UNESCAPES: Record<string, string> = {
  dblquote: '"',
  quote: "'",
  lt: "<",
  gt: ">",
  backslash: "\\",
  slash: "/",
  bar: "|",
  comma: ",",
  colon: ":",
  space: " ",
  dollar: "$",
  tab: "\t",
  return: "\n",
  brace: "{",
};

/** `UnescapeString`: `{slash}` and friends back to their characters; markup like `~{OVER}` is left as written. */
export function unescapeString(s: string): string {
  if (s.length <= 2) return s;
  let out = "";
  let prev = "";
  let ch = "";
  for (let i = 0; i < s.length; i++) {
    prev = ch;
    ch = s[i]!;
    if (ch !== "{") {
      out += ch;
      continue;
    }
    let token = "";
    let depth = 1;
    let terminated = false;
    for (i = i + 1; i < s.length; i++) {
      ch = s[i]!;
      if (ch === "{") depth++;
      else if (ch === "}") depth--;
      if (depth <= 0) {
        terminated = true;
        break;
      }
      token += ch;
    }
    if (!terminated) out += `{${unescapeString(token)}`;
    else if (prev === "$" || prev === "~" || prev === "^" || prev === "_") out += `{${unescapeString(token)}}`;
    else if (token in UNESCAPES) out += UNESCAPES[token]!;
    else out += `{${unescapeString(token)}}`;
  }
  return out;
}

/**
 * `NET_SETTINGS::ParseBusGroup` reduced to its yes/no: a bus group is an optional name followed by `{member member ...}`
 * to the end of the text, the opening brace not being a markup brace (`~{`, `^{`, `_{`, `${`).
 */
export function isBusGroup(text: string): boolean {
  if (!text.endsWith("}")) return false;
  for (let i = 0; i < text.length; i++) {
    if (text[i] !== "{") continue;
    const prev = i > 0 ? text[i - 1]! : "";
    if (prev === "$" || prev === "~" || prev === "^" || prev === "_") continue;
    let depth = 0;
    for (let j = i; j < text.length; j++) {
      if (text[j] === "{") depth++;
      else if (text[j] === "}") depth--;
      if (depth === 0) return j === text.length - 1;
    }
    return false;
  }
  return false;
}

/** The `<empty>` text a label made from nothing gets (`_( "<empty>" )`). */
export const EMPTY_LABEL_TEXT = "<empty>";

/**
 * `getValidNetname`: line breaks and tabs become `_`; spaces too, unless the text is a bus group (whose members are
 * separated by spaces); then the netname escape; an empty result is `<empty>`.
 */
export function validNetName(text: string): string {
  let t = text.replace(/\n/g, "_").replace(/\r/g, "_").replace(/\t/g, "_");
  if (!isBusGroup(text)) t = t.replace(/ /g, "_");
  t = escapeNetname(t);
  return t === "" ? EMPTY_LABEL_TEXT : t;
}
