// `HTML_MESSAGE_BOX` (common/dialogs/html_message_box.cpp at KiCad 8303b2ad): the window KiCad shows a page of HTML in -- the Syntax Help of the
// rules editors among others. What it does besides showing the page: a link to KiCad's documentation gets the version put in
// (`OnHTMLLinkClicked`), and typing in the window opens a search bar that marks every match in the page and steps through them (`OnCharHook`,
// `updateSearch`, `OnNext`, `OnPrev`). The window itself is components/HtmlMessageBox.tsx.
//
// Pure: no React, no DOM.

/** `OnHTMLLinkClicked`: a link into `https://go.kicad.org/docs` has its `GetMajorMinorVersion` placeholder replaced by the version. */
export function htmlLinkHref(href: string, majorMinorVersion: string): string {
  return href.startsWith("https://go.kicad.org/docs") ? href.replaceAll("GetMajorMinorVersion", majorMinorVersion) : href;
}

/** The page with the matches of a search marked, and where the search stands. */
export interface HtmlSearch {
  /** The page to show: `m_originalSource` with each match wrapped (or the page as it was when there is nothing to show). */
  html: string;
  /** Where each match starts in the original page (`m_matchPos`). */
  matches: number[];
  /** The match the window scrolls to (`m_currentMatch`), within `matches`; 0 when there are none. */
  current: number;
  /** The label beside the search box: "" for no term, "0/0" for no match, else "n/m". */
  label: string;
  /** The anchor at the current match (`kicad_search_<n>`), or null. */
  anchor: string | null;
}

/** `escapedMatchStr`: the four characters `updateSearch` escapes in a match. */
function escapeMatch(text: string): string {
  return text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}

/**
 * `HTML_MESSAGE_BOX::updateSearch`: the matches of `term` (case ignored) in the text of `source` outside its tags -- found character by
 * character in the HTML as it is, so an entity is matched as written (`&lt;`, not `<`) -- each wrapped in a yellow span, the current one in a
 * violet span after an anchor named `kicad_search_<current>`. `current` is `m_currentMatch` before the call; past the last match it goes back to 0.
 */
export function searchHtml(source: string, term: string, current: number): HtmlSearch {
  if (term === "") return { html: source, matches: [], current: 0, label: "", anchor: null };
  const termLower = term.toLowerCase();
  const matches: number[] = [];
  let insideTag = false;
  for (let pos = 0; pos < source.length; pos++) {
    const ch = source[pos];
    if (ch === "<") insideTag = true;
    else if (ch === ">") {
      insideTag = false;
      continue;
    }
    // Only search for matches when we're not inside an HTML tag
    if (insideTag) continue;
    if (pos + termLower.length <= source.length && source.slice(pos, pos + termLower.length).toLowerCase() === termLower) {
      // Verify that this match doesn't span into an HTML tag
      if (!source.slice(pos, pos + termLower.length).includes("<")) matches.push(pos);
    }
  }
  if (matches.length === 0) return { html: source, matches, current: 0, label: "0/0", anchor: null };
  const cur = current >= matches.length ? 0 : current;
  let out = "";
  let start = 0;
  matches.forEach((idx, i) => {
    out += source.slice(start, idx);
    const matchStr = escapeMatch(source.slice(idx, idx + term.length));
    if (i === cur) out += `<a name="kicad_search_${cur}"></a><span style="background-color:#DDAAFF;">${matchStr}</span>`;
    else out += `<span style="background-color:#FFFFAA;">${matchStr}</span>`;
    start = idx + term.length;
  });
  out += source.slice(start);
  return { html: out, matches, current: cur, label: `${cur + 1}/${matches.length}`, anchor: `kicad_search_${cur}` };
}

/** `OnNext`: the match after the current one, round to the first. */
export const nextMatch = (current: number, count: number): number => (count > 0 ? (current + 1) % count : current);

/** `OnPrev`: the match before the current one, round to the last. */
export const prevMatch = (current: number, count: number): number => (count > 0 ? (current + count - 1) % count : current);
