// `ConvertMarkdown2Html` (common/markdown2html.cpp at KiCad 8303b2ad: thirdparty/markdown2html/markdown2html.cpp) -- the markdown KiCad's help windows
// are written in (the Syntax Help of Board Setup > Custom Rules and of the DRC Rule Editor, `panel_setup_rules.cpp::OnSyntaxHelp`), turned into the HTML
// its `HTML_MESSAGE_BOX` shows. KiCad's converter is the bundled sundown 1.16 (thirdparty/markdown2html/md_parser/markdown.c, the parser, and
// html_formatter/html.c with houdini_html_e.c / houdini_href_e.c, the HTML renderer), run as `sd_markdown_new( MKDEXT_TABLES, 16, ... )` over
// `sdhtml_renderer( ..., 0 )`: tables on, every other extension off (no fenced code, no autolinking of bare URLs, no strikethrough, no superscript,
// no lax spacing, intra-word emphasis allowed), and no render flags (raw HTML passes through, nothing is escaped that markdown does not escape,
// HTML 4 `<br>` and `<hr>`). This is that parser and renderer, function for function, with the branches those switches turn off left out.
// The 8303b2ad tree on this box has no thirdparty/ directory; the sources were read at KiCad 25ec2335, where git shows them unchanged since 8303b2ad.
// web/studio/tools/sundown-golden.mjs compiles KiCad's C and writes what it renders for the help texts and a set of samples to
// markdownHtml.golden.json; markdownHtml.test.ts holds this port to it byte for byte.
//
// Where this differs: sundown works on UTF-8 bytes, this on the string's UTF-16 units, so a character outside ASCII counts as one in the tab
// expansion where KiCad counts its bytes (KiCad's help texts are ASCII); an href is still escaped byte by byte of its UTF-8, as houdini does.
//
// Pure: no React, no DOM.

const MKD_LIST_ORDERED = 1;
const MKD_LI_BLOCK = 2;
const MKD_LI_END = 8;
const MKD_TABLE_ALIGN_L = 1;
const MKD_TABLE_ALIGN_R = 2;
const MKD_TABLE_ALIGN_CENTER = 3;
const MKD_TABLE_ALIGNMASK = 3;
const MKD_TABLE_HEADER = 4;

const enum Autolink {
  NotAutolink,
  Normal,
  Email,
}

/** `max_nesting`: `sd_markdown_new( MKDEXT_TABLES, 16, ... )`. */
const MAX_NESTING = 16;

/** html_blocks.h `find_block_tag`: the block-level tags (gperf over a case-insensitive compare). */
const BLOCK_TAGS = new Set([
  "p", "dl", "div", "math", "table", "ul", "del", "form", "blockquote", "figure", "ol", "fieldset",
  "h1", "h6", "pre", "script", "h5", "noscript", "style", "iframe", "h4", "ins", "h3", "h2",
]);

/** `_isspace`: a markdown space (tabs and carriage returns are gone after the first pass). */
const mdSpace = (c: string | undefined): boolean => c === " " || c === "\n";
/** C `isspace`. */
const cSpace = (c: string | undefined): boolean => c === " " || c === "\t" || c === "\n" || c === "\v" || c === "\f" || c === "\r";
/** C `isalnum` (the "C" locale: ASCII only). */
const cAlnum = (c: string | undefined): boolean => c !== undefined && /^[0-9A-Za-z]$/.test(c);
const isDigit = (c: string | undefined): boolean => c !== undefined && c >= "0" && c <= "9";

/** `struct buf`: the output being written. */
class Buf {
  s = "";
  get size(): number {
    return this.s.length;
  }
  put(text: string): void {
    this.s += text;
  }
  truncate(size: number): void {
    this.s = this.s.slice(0, size);
  }
  last(): string | undefined {
    return this.s[this.s.length - 1];
  }
}

/** A view into the text: `data + o`, as the C passes `uint8_t* data, size_t size`. */
const at = (d: string, o: number, i: number): string | undefined => (o + i >= 0 ? d[o + i] : undefined);

// ------------------------------------------------------------------------------------------------------------------- houdini (html.c escapers)

/** `houdini_escape_html0( ob, src, size, 0 )`: `& < > " '` (the forward slash only in secure mode, which html.c does not ask for). */
function escapeHtml(ob: Buf, text: string): void {
  ob.put(text.replace(/["&'<>]/g, (c) => (c === '"' ? "&quot;" : c === "&" ? "&amp;" : c === "'" ? "&#39;" : c === "<" ? "&lt;" : "&gt;")));
}

/** houdini_href_e.c `HREF_SAFE`: `-_.+!*'(),%#@?=;:/,+&$` and the alphanumerics are left alone (`&` and `'` get an entity). */
const HREF_SAFE = /^[!#$%()*+,\-./0-9:;=?@A-Z_a-z]$/;

/** `houdini_escape_href`: an URL inside `href="..."`: `&` -> `&amp;`, `'` -> `&#x27;`, anything else not safe -> `%XX` of each UTF-8 byte. */
function escapeHref(ob: Buf, text: string): void {
  let out = "";
  for (const ch of text) {
    if (HREF_SAFE.test(ch)) out += ch;
    else if (ch === "&") out += "&amp;";
    else if (ch === "'") out += "&#x27;";
    else for (const b of new TextEncoder().encode(ch)) out += `%${b.toString(16).toUpperCase().padStart(2, "0")}`;
  }
  ob.put(out);
}

// ------------------------------------------------------------------------------------------------------------------------ the HTML renderer

/** html.c's callbacks with `render_flags` 0 (`sdhtml_renderer`). The span ones answer whether they rendered (0 prints the span as it is). */
const html = {
  blockcode(ob: Buf, text: string): void {
    if (ob.size) ob.put("\n");
    ob.put("<pre><code>");
    escapeHtml(ob, text);
    ob.put("</code></pre>\n");
  },
  blockquote(ob: Buf, text: string): void {
    if (ob.size) ob.put("\n");
    ob.put("<blockquote>\n");
    ob.put(text);
    ob.put("</blockquote>\n");
  },
  /** `rndr_raw_block`. */
  blockhtml(ob: Buf, text: string): void {
    let sz = text.length;
    while (sz > 0 && text[sz - 1] === "\n") sz--;
    let org = 0;
    while (org < sz && text[org] === "\n") org++;
    if (org >= sz) return;
    if (ob.size) ob.put("\n");
    ob.put(text.slice(org, sz));
    ob.put("\n");
  },
  header(ob: Buf, text: string, level: number): void {
    if (ob.size) ob.put("\n");
    ob.put(`<h${level}>`);
    ob.put(text);
    ob.put(`</h${level}>\n`);
  },
  hrule(ob: Buf): void {
    if (ob.size) ob.put("\n");
    ob.put("<hr>\n");
  },
  list(ob: Buf, text: string, flags: number): void {
    if (ob.size) ob.put("\n");
    ob.put(flags & MKD_LIST_ORDERED ? "<ol>\n" : "<ul>\n");
    ob.put(text);
    ob.put(flags & MKD_LIST_ORDERED ? "</ol>\n" : "</ul>\n");
  },
  listitem(ob: Buf, text: string): void {
    ob.put("<li>");
    let size = text.length;
    while (size && text[size - 1] === "\n") size--;
    ob.put(text.slice(0, size));
    ob.put("</li>\n");
  },
  paragraph(ob: Buf, text: string): void {
    if (ob.size) ob.put("\n");
    if (!text.length) return;
    let i = 0;
    while (i < text.length && cSpace(text[i])) i++;
    if (i === text.length) return;
    ob.put("<p>");
    ob.put(text.slice(i));
    ob.put("</p>\n");
  },
  table(ob: Buf, header: string, body: string): void {
    if (ob.size) ob.put("\n");
    ob.put("<table><thead>\n");
    ob.put(header);
    ob.put("</thead><tbody>\n");
    ob.put(body);
    ob.put("</tbody></table>\n");
  },
  tableRow(ob: Buf, text: string): void {
    ob.put("<tr>\n");
    ob.put(text);
    ob.put("</tr>\n");
  },
  tableCell(ob: Buf, text: string, flags: number): void {
    ob.put(flags & MKD_TABLE_HEADER ? "<th" : "<td");
    switch (flags & MKD_TABLE_ALIGNMASK) {
      case MKD_TABLE_ALIGN_CENTER:
        ob.put(' align="center">');
        break;
      case MKD_TABLE_ALIGN_L:
        ob.put(' align="left">');
        break;
      case MKD_TABLE_ALIGN_R:
        ob.put(' align="right">');
        break;
      default:
        ob.put(">");
    }
    ob.put(text);
    ob.put(flags & MKD_TABLE_HEADER ? "</th>\n" : "</td>\n");
  },
  autolink(ob: Buf, link: string, type: Autolink): boolean {
    if (!link.length) return false;
    ob.put('<a href="');
    if (type === Autolink.Email) ob.put("mailto:");
    escapeHref(ob, link);
    ob.put('">');
    // `bufprefix( link, "mailto:" ) == 0`: an address written as a mailto: URI shows without the scheme.
    escapeHtml(ob, link.startsWith("mailto:") ? link.slice(7) : link);
    ob.put("</a>");
    return true;
  },
  codespan(ob: Buf, text: string | null): boolean {
    ob.put("<code>");
    if (text !== null) escapeHtml(ob, text);
    ob.put("</code>");
    return true;
  },
  doubleEmphasis(ob: Buf, text: string): boolean {
    if (!text.length) return false;
    ob.put("<strong>" + text + "</strong>");
    return true;
  },
  emphasis(ob: Buf, text: string): boolean {
    if (!text.length) return false;
    ob.put("<em>" + text + "</em>");
    return true;
  },
  tripleEmphasis(ob: Buf, text: string): boolean {
    if (!text.length) return false;
    ob.put("<strong><em>" + text + "</em></strong>");
    return true;
  },
  image(ob: Buf, link: string | null, title: string | null, alt: string | null): boolean {
    if (!link) return false;
    ob.put('<img src="');
    escapeHref(ob, link);
    ob.put('" alt="');
    if (alt) escapeHtml(ob, alt);
    if (title) {
      ob.put('" title="');
      escapeHtml(ob, title);
    }
    ob.put('">');
    return true;
  },
  linebreak(ob: Buf): boolean {
    ob.put("<br>\n");
    return true;
  },
  link(ob: Buf, link: string | null, title: string | null, content: string | null): boolean {
    ob.put('<a href="');
    if (link) escapeHref(ob, link);
    if (title) {
      ob.put('" title="');
      escapeHtml(ob, title);
    }
    ob.put('">');
    if (content) ob.put(content);
    ob.put("</a>");
    return true;
  },
  /** `rndr_raw_html`: no HTML_ESCAPE / HTML_SKIP_* flags, so the tag goes out as it is. */
  rawHtml(ob: Buf, text: string): boolean {
    ob.put(text);
    return true;
  },
  normalText(ob: Buf, text: string): void {
    escapeHtml(ob, text);
  },
};

// ------------------------------------------------------------------------------------------------------------------------------- the parser

type Trigger = (ob: Buf, d: string, o: number, offset: number, size: number) => number;

/** `struct sd_markdown`: one render. */
class Markdown {
  private refs = new Map<number, { link: string; title: string | null }>();
  /** `work_bufs[BUFFER_SPAN].size + work_bufs[BUFFER_BLOCK].size`: the buffers in use, against `max_nesting`. */
  private nesting = 0;
  /** `active_char` / `markdown_char_ptrs`: the characters that start a span (html.c has every span callback; no autolink, superscript or strikethrough extension). */
  private readonly triggers = new Map<string, Trigger>([
    ["*", (ob, d, o, _offset, size) => this.charEmphasis(ob, d, o, size)],
    ["_", (ob, d, o, _offset, size) => this.charEmphasis(ob, d, o, size)],
    ["`", (ob, d, o, _offset, size) => this.charCodespan(ob, d, o, size)],
    ["\n", (ob, d, o, offset) => this.charLinebreak(ob, d, o, offset)],
    ["[", (ob, d, o, offset, size) => this.charLink(ob, d, o, offset, size)],
    ["<", (ob, d, o, _offset, size) => this.charLangleTag(ob, d, o, size)],
    ["\\", (ob, d, o, _offset, size) => this.charEscape(ob, d, o, size)],
    ["&", (ob, d, o, _offset, size) => this.charEntity(ob, d, o, size)],
  ]);

  /** `rndr_newbuf`: a fresh output buffer, counted against the nesting limit until `rndr_popbuf`. */
  private withBuf<T>(fn: (b: Buf) => T): T {
    this.nesting++;
    try {
      return fn(new Buf());
    } finally {
      this.nesting--;
    }
  }

  // ---------------------------------------------------------------------------------------------------------------------------- inline

  /** `parse_inline`. */
  parseInline(ob: Buf, d: string, o: number, size: number): void {
    if (this.nesting > MAX_NESTING) return;
    let i = 0;
    let end = 0;
    let action: Trigger | undefined;
    while (i < size) {
      while (end < size && (action = this.triggers.get(at(d, o, end)!)) === undefined) end++;
      html.normalText(ob, d.slice(o + i, o + end));
      if (end >= size) break;
      i = end;
      end = action!(ob, d, o + i, i, size - i);
      if (!end) end = i + 1;
      else {
        i += end;
        end = i;
      }
    }
  }

  /** `find_emph_char`: the next `c`, skipping code spans and links. */
  private findEmphChar(d: string, o: number, size: number, c: string): number {
    let i = 1;
    while (i < size) {
      while (i < size && at(d, o, i) !== c && at(d, o, i) !== "`" && at(d, o, i) !== "[") i++;
      if (i === size) return 0;
      if (at(d, o, i) === c) return i;
      // not counting escaped chars
      if (i && at(d, o, i - 1) === "\\") {
        i++;
        continue;
      }
      if (at(d, o, i) === "`") {
        let spanNb = 0;
        let tmpI = 0;
        while (i < size && at(d, o, i) === "`") {
          i++;
          spanNb++;
        }
        if (i >= size) return 0;
        let bt = 0;
        while (i < size && bt < spanNb) {
          if (!tmpI && at(d, o, i) === c) tmpI = i;
          if (at(d, o, i) === "`") bt++;
          else bt = 0;
          i++;
        }
        if (i >= size) return tmpI;
      } else if (at(d, o, i) === "[") {
        let tmpI = 0;
        i++;
        while (i < size && at(d, o, i) !== "]") {
          if (!tmpI && at(d, o, i) === c) tmpI = i;
          i++;
        }
        i++;
        while (i < size && (at(d, o, i) === " " || at(d, o, i) === "\n")) i++;
        if (i >= size) return tmpI;
        let cc: string;
        switch (at(d, o, i)) {
          case "[":
            cc = "]";
            break;
          case "(":
            cc = ")";
            break;
          default:
            if (tmpI) return tmpI;
            continue;
        }
        i++;
        while (i < size && at(d, o, i) !== cc) {
          if (!tmpI && at(d, o, i) === c) tmpI = i;
          i++;
        }
        if (i >= size) return tmpI;
        i++;
      }
    }
    return 0;
  }

  /** `parse_emph1`: single emphasis, closed by a symbol not preceded by whitespace. */
  private parseEmph1(ob: Buf, d: string, o: number, size: number, c: string): number {
    let i = 0;
    // skipping one symbol if coming from emph3
    if (size > 1 && at(d, o, 0) === c && at(d, o, 1) === c) i = 1;
    while (i < size) {
      const len = this.findEmphChar(d, o + i, size - i, c);
      if (!len) return 0;
      i += len;
      if (i >= size) return 0;
      if (at(d, o, i) === c && !mdSpace(at(d, o, i - 1))) {
        const end = i;
        const r = this.withBuf((work) => {
          this.parseInline(work, d, o, end);
          return html.emphasis(ob, work.s);
        });
        return r ? i + 1 : 0;
      }
    }
    return 0;
  }

  /** `parse_emph2`: double emphasis. */
  private parseEmph2(ob: Buf, d: string, o: number, size: number, c: string): number {
    let i = 0;
    while (i < size) {
      const len = this.findEmphChar(d, o + i, size - i, c);
      if (!len) return 0;
      i += len;
      if (i + 1 < size && at(d, o, i) === c && at(d, o, i + 1) === c && i && !mdSpace(at(d, o, i - 1))) {
        const end = i;
        const r = this.withBuf((work) => {
          this.parseInline(work, d, o, end);
          return html.doubleEmphasis(ob, work.s);
        });
        return r ? i + 2 : 0;
      }
      i++;
    }
    return 0;
  }

  /** `parse_emph3`: finds the first closing tag and hands over to the other emphases. */
  private parseEmph3(ob: Buf, d: string, o: number, size: number, c: string): number {
    let i = 0;
    while (i < size) {
      const len = this.findEmphChar(d, o + i, size - i, c);
      if (!len) return 0;
      i += len;
      // skip whitespace preceded symbols
      if (at(d, o, i) !== c || mdSpace(at(d, o, i - 1))) continue;
      if (i + 2 < size && at(d, o, i + 1) === c && at(d, o, i + 2) === c) {
        const end = i;
        const r = this.withBuf((work) => {
          this.parseInline(work, d, o, end);
          return html.tripleEmphasis(ob, work.s);
        });
        return r ? i + 3 : 0;
      } else if (i + 1 < size && at(d, o, i + 1) === c) {
        // double symbol found, handing over to emph1
        const l = this.parseEmph1(ob, d, o - 2, size + 2, c);
        return l ? l - 2 : 0;
      } else {
        // single symbol found, handing over to emph2
        const l = this.parseEmph2(ob, d, o - 1, size + 1, c);
        return l ? l - 1 : 0;
      }
    }
    return 0;
  }

  /** `char_emphasis`: `*` and `_`. */
  private charEmphasis(ob: Buf, d: string, o: number, size: number): number {
    const c = at(d, o, 0)!;
    let ret: number;
    if (size > 2 && at(d, o, 1) !== c) {
      // whitespace cannot follow an opening emphasis
      if (mdSpace(at(d, o, 1)) || (ret = this.parseEmph1(ob, d, o + 1, size - 1, c)) === 0) return 0;
      return ret + 1;
    }
    if (size > 3 && at(d, o, 1) === c && at(d, o, 2) !== c) {
      if (mdSpace(at(d, o, 2)) || (ret = this.parseEmph2(ob, d, o + 2, size - 2, c)) === 0) return 0;
      return ret + 2;
    }
    if (size > 4 && at(d, o, 1) === c && at(d, o, 2) === c && at(d, o, 3) !== c) {
      if (mdSpace(at(d, o, 3)) || (ret = this.parseEmph3(ob, d, o + 3, size - 3, c)) === 0) return 0;
      return ret + 3;
    }
    return 0;
  }

  /** `char_linebreak`: a newline after two spaces. */
  private charLinebreak(ob: Buf, d: string, o: number, offset: number): number {
    if (offset < 2 || at(d, o, -1) !== " " || at(d, o, -2) !== " ") return 0;
    // removing the last space from ob and rendering
    let n = ob.size;
    while (n && ob.s[n - 1] === " ") n--;
    ob.truncate(n);
    return html.linebreak(ob) ? 1 : 0;
  }

  /** `char_codespan`. */
  private charCodespan(ob: Buf, d: string, o: number, size: number): number {
    let nb = 0;
    while (nb < size && at(d, o, nb) === "`") nb++;
    let i = 0;
    let end: number;
    for (end = nb; end < size && i < nb; end++) {
      if (at(d, o, end) === "`") i++;
      else i = 0;
    }
    if (i < nb && end >= size) return 0; // no matching delimiter
    // trimming outside whitespaces
    let fBegin = nb;
    while (fBegin < end && at(d, o, fBegin) === " ") fBegin++;
    let fEnd = end - nb;
    while (fEnd > nb && at(d, o, fEnd - 1) === " ") fEnd--;
    const ok = fBegin < fEnd ? html.codespan(ob, d.slice(o + fBegin, o + fEnd)) : html.codespan(ob, null);
    return ok ? end : 0;
  }

  /** `char_escape`: a backslash before one of ``\`*_{}[]()#+-.!:|&<>^~``. */
  private charEscape(ob: Buf, d: string, o: number, size: number): number {
    if (size > 1) {
      if (!"\\`*_{}[]()#+-.!:|&<>^~".includes(at(d, o, 1)!)) return 0;
      html.normalText(ob, at(d, o, 1)!);
    } else if (size === 1) {
      ob.put(at(d, o, 0)!);
    }
    return 2;
  }

  /** `char_entity`: `&#?[A-Za-z0-9]+;` goes out as it is (html.c has no entity callback); a lone `&` is text. */
  private charEntity(ob: Buf, d: string, o: number, size: number): number {
    let end = 1;
    if (end < size && at(d, o, end) === "#") end++;
    while (end < size && cAlnum(at(d, o, end))) end++;
    if (end < size && at(d, o, end) === ";") end++;
    else return 0;
    ob.put(d.slice(o, o + end));
    return end;
  }

  /** `char_langle_tag`: `<http://...>`, `<a@b.c>` or a raw tag. */
  private charLangleTag(ob: Buf, d: string, o: number, size: number): number {
    const t = tagLength(d, o, size);
    let ret = false;
    if (t.end > 2) {
      if (t.autolink !== Autolink.NotAutolink) {
        ret = this.withBuf(() => html.autolink(ob, unescapeText(d.slice(o + 1, o + t.end - 1)), t.autolink));
      } else {
        ret = html.rawHtml(ob, d.slice(o, o + t.end));
      }
    }
    return ret ? t.end : 0;
  }

  /** `char_link`: `[text](url "title")`, `[text][id]`, `[id]`, and the same after `!` for an image. */
  private charLink(ob: Buf, d: string, o: number, offset: number, size: number): number {
    const isImg = offset > 0 && at(d, o, -1) === "!";
    let i = 1;
    let txtE: number;
    let linkB = 0;
    let linkE = 0;
    let titleB = 0;
    let titleE = 0;
    let link: string | null = null;
    let title: string | null = null;
    let textHasNl = false;
    // `org_work_size`: the buffers taken for the link, its title, its id and its content are given back at `cleanup`.
    const nesting = this.nesting;
    try {
      // looking for the matching closing bracket
      for (let level = 1; i < size; i++) {
        if (at(d, o, i) === "\n") textHasNl = true;
        else if (at(d, o, i - 1) === "\\") continue;
        else if (at(d, o, i) === "[") level++;
        else if (at(d, o, i) === "]") {
          level--;
          if (level <= 0) break;
        }
      }
      if (i >= size) return 0;
      txtE = i;
      i++;
      // skip any amount of whitespace or newline (this is much more laxist than original markdown syntax)
      while (i < size && mdSpace(at(d, o, i))) i++;

      const craftedId = (): string => {
        if (!textHasNl) return d.slice(o + 1, o + txtE);
        this.nesting++; // its `rndr_newbuf`
        let b = "";
        for (let j = 1; j < txtE; j++) {
          if (at(d, o, j) !== "\n") b += at(d, o, j);
          else if (at(d, o, j - 1) !== " ") b += " ";
        }
        return b;
      };

      if (i < size && at(d, o, i) === "(") {
        // inline style link
        i++;
        while (i < size && mdSpace(at(d, o, i))) i++;
        linkB = i;
        // looking for link end: ' " )
        while (i < size) {
          if (at(d, o, i) === "\\") i += 2;
          else if (at(d, o, i) === ")") break;
          else if (i >= 1 && mdSpace(at(d, o, i - 1)) && (at(d, o, i) === "'" || at(d, o, i) === '"')) break;
          else i++;
        }
        if (i >= size) return 0;
        linkE = i;
        // looking for title end if present
        if (at(d, o, i) === "'" || at(d, o, i) === '"') {
          const qtype = at(d, o, i);
          let inTitle = true;
          i++;
          titleB = i;
          while (i < size) {
            if (at(d, o, i) === "\\") i += 2;
            else if (at(d, o, i) === qtype) {
              inTitle = false;
              i++;
            } else if (at(d, o, i) === ")" && !inTitle) break;
            else i++;
          }
          if (i >= size) return 0;
          // skipping whitespaces after title
          titleE = i - 1;
          while (titleE > titleB && mdSpace(at(d, o, titleE))) titleE--;
          // checking for closing quote presence
          if (at(d, o, titleE) !== "'" && at(d, o, titleE) !== '"') {
            titleB = titleE = 0;
            linkE = i;
          }
        }
        // remove whitespace at the end of the link
        while (linkE > linkB && mdSpace(at(d, o, linkE - 1))) linkE--;
        // remove optional angle brackets around the link
        if (at(d, o, linkB) === "<") linkB++;
        if (at(d, o, linkE - 1) === ">") linkE--;
        if (linkE > linkB) {
          link = d.slice(o + linkB, o + linkE);
          this.nesting++;
        }
        if (titleE > titleB) {
          title = d.slice(o + titleB, o + titleE);
          this.nesting++;
        }
        i++;
      } else if (i < size && at(d, o, i) === "[") {
        // reference style link
        i++;
        linkB = i;
        while (i < size && at(d, o, i) !== "]") i++;
        if (i >= size) return 0;
        linkE = i;
        const id = linkB === linkE ? craftedId() : d.slice(o + linkB, o + linkE);
        const lr = this.refs.get(hashLinkRef(id));
        if (!lr) return 0;
        link = lr.link;
        title = lr.title;
        i++;
      } else {
        // shortcut reference style link
        const lr = this.refs.get(hashLinkRef(craftedId()));
        if (!lr) return 0;
        link = lr.link;
        title = lr.title;
        // rewinding the whitespace
        i = txtE + 1;
      }

      // building content: img alt is escaped, link content is parsed
      let content: string | null = null;
      if (txtE > 1) {
        if (isImg) content = d.slice(o + 1, o + txtE);
        else {
          // (sundown turns autolinking off here, `in_link_body`; it is off anyway without MKDEXT_AUTOLINK)
          const b = new Buf();
          this.nesting++;
          this.parseInline(b, d, o + 1, txtE - 1);
          content = b.s;
        }
      }
      const uLink = link !== null ? unescapeText(link) : null;
      let ret: boolean;
      if (isImg) {
        if (ob.size && ob.last() === "!") ob.truncate(ob.size - 1);
        ret = html.image(ob, uLink, title, content);
      } else {
        ret = html.link(ob, uLink, title, content);
      }
      return ret ? i : 0;
    } finally {
      this.nesting = nesting;
    }
  }

  // ------------------------------------------------------------------------------------------------------------------------------ blocks

  /** `parse_blockquote`: the lines without their `>`, parsed as blocks. */
  private parseBlockquote(ob: Buf, d: string, o: number, size: number): number {
    let beg = 0;
    let end = 0;
    let work = "";
    while (beg < size) {
      for (end = beg + 1; end < size && at(d, o, end - 1) !== "\n"; end++);
      const pre = prefixQuote(d, o + beg, end - beg);
      if (pre) beg += pre; // skipping prefix
      // empty line followed by non-quote line
      else if (isEmpty(d, o + beg, end - beg) && (end >= size || (prefixQuote(d, o + end, size - end) === 0 && !isEmpty(d, o + end, size - end)))) break;
      if (beg < end) work += d.slice(o + beg, o + end);
      beg = end;
    }
    this.withBuf((out) => {
      this.parseBlock(out, work, 0, work.length);
      html.blockquote(ob, out.s);
    });
    return end;
  }

  /** `parse_paragraph` (and a setext header: a paragraph whose last line is underlined). */
  private parseParagraph(ob: Buf, d: string, o: number, size: number): number {
    let i = 0;
    let end = 0;
    let level = 0;
    while (i < size) {
      for (end = i + 1; end < size && at(d, o, end - 1) !== "\n"; end++);
      if (isEmpty(d, o + i, size - i)) break;
      if ((level = isHeaderline(d, o + i, size - i)) !== 0) break;
      if (isAtxheader(d, o + i) || isHrule(d, o + i, size - i) || prefixQuote(d, o + i, size - i)) {
        end = i;
        break;
      }
      i = end;
    }
    let workStart = o;
    let workSize = i;
    while (workSize && at(d, o, workSize - 1) === "\n") workSize--;
    if (!level) {
      this.withBuf((tmp) => {
        this.parseInline(tmp, d, workStart, workSize);
        html.paragraph(ob, tmp.s);
      });
    } else {
      if (workSize) {
        i = workSize;
        workSize -= 1;
        while (workSize && at(d, o, workSize) !== "\n") workSize -= 1;
        const beg = workSize + 1;
        while (workSize && at(d, o, workSize - 1) === "\n") workSize -= 1;
        if (workSize > 0) {
          const n = workSize;
          this.withBuf((tmp) => {
            this.parseInline(tmp, d, o, n);
            html.paragraph(ob, tmp.s);
          });
          workStart = o + beg;
          workSize = i - beg;
        } else workSize = i;
      }
      this.withBuf((headerWork) => {
        this.parseInline(headerWork, d, workStart, workSize);
        html.header(ob, headerWork.s, level);
      });
    }
    return end;
  }

  /** `parse_blockcode`: lines indented by four spaces. */
  private parseBlockcode(ob: Buf, d: string, o: number, size: number): number {
    let beg = 0;
    let work = "";
    while (beg < size) {
      let end: number;
      for (end = beg + 1; end < size && at(d, o, end - 1) !== "\n"; end++);
      const pre = prefixCode(d, o + beg, end - beg);
      if (pre) beg += pre; // skipping prefix
      else if (!isEmpty(d, o + beg, end - beg)) break; // non-empty non-prefixed line breaks the pre
      if (beg < end) {
        // verbatim copy to the working buffer, escaping entities
        if (isEmpty(d, o + beg, end - beg)) work += "\n";
        else work += d.slice(o + beg, o + end);
      }
      beg = end;
    }
    while (work.length && work[work.length - 1] === "\n") work = work.slice(0, -1);
    work += "\n";
    html.blockcode(ob, work);
    return beg;
  }

  /** `parse_listitem`: one item, its prefix still on; `flags` is shared with the list's other items. */
  private parseListitem(ob: Buf, d: string, o: number, size: number, flags: { v: number }): number {
    let orgpre = 0;
    while (orgpre < 3 && orgpre < size && at(d, o, orgpre) === " ") orgpre++;
    let beg = prefixUli(d, o, size);
    if (!beg) beg = prefixOli(d, o, size);
    if (!beg) return 0;
    // skipping to the beginning of the following line
    let end = beg;
    while (end < size && at(d, o, end - 1) !== "\n") end++;
    this.nesting += 2; // `work` and `inter`
    try {
      let work = d.slice(o + beg, o + end);
      const inter = new Buf();
      let sublist = 0;
      let inEmpty = false;
      let hasInsideEmpty = false;
      beg = end;
      while (beg < size) {
        end++;
        while (end < size && at(d, o, end - 1) !== "\n") end++;
        // process an empty line
        if (isEmpty(d, o + beg, end - beg)) {
          inEmpty = true;
          beg = end;
          continue;
        }
        // calculating the indentation
        let i = 0;
        while (i < 4 && beg + i < end && at(d, o, beg + i) === " ") i++;
        const pre = i;
        const hasNextUli = prefixUli(d, o + beg + i, end - beg - i);
        const hasNextOli = prefixOli(d, o + beg + i, end - beg - i);
        // checking for ul/ol switch
        if (inEmpty && ((flags.v & MKD_LIST_ORDERED && hasNextUli) || (!(flags.v & MKD_LIST_ORDERED) && hasNextOli))) {
          flags.v |= MKD_LI_END;
          break; // the following item must have same list type
        }
        // checking for a new item
        if ((hasNextUli && !isHrule(d, o + beg + i, end - beg - i)) || hasNextOli) {
          if (inEmpty) hasInsideEmpty = true;
          if (pre === orgpre) break; // the following item must have the same indentation
          if (!sublist) sublist = work.length;
        } else if (inEmpty && pre === 0) {
          // joining only indented stuff after empty lines; note that now we only require 1 space of indentation to continue a list
          flags.v |= MKD_LI_END;
          break;
        } else if (inEmpty) {
          work += "\n";
          hasInsideEmpty = true;
        }
        inEmpty = false;
        // adding the line without prefix into the working buffer
        work += d.slice(o + beg + i, o + end);
        beg = end;
      }
      // render of li contents
      if (hasInsideEmpty) flags.v |= MKD_LI_BLOCK;
      if (flags.v & MKD_LI_BLOCK) {
        // intermediate render of block li
        if (sublist && sublist < work.length) {
          this.parseBlock(inter, work, 0, sublist);
          this.parseBlock(inter, work, sublist, work.length - sublist);
        } else this.parseBlock(inter, work, 0, work.length);
      } else {
        // intermediate render of inline li
        if (sublist && sublist < work.length) {
          this.parseInline(inter, work, 0, sublist);
          this.parseBlock(inter, work, sublist, work.length - sublist);
        } else this.parseInline(inter, work, 0, work.length);
      }
      html.listitem(ob, inter.s);
      return beg;
    } finally {
      this.nesting -= 2;
    }
  }

  /** `parse_list`. */
  private parseList(ob: Buf, d: string, o: number, size: number, flags0: number): number {
    const flags = { v: flags0 };
    return this.withBuf((work) => {
      let i = 0;
      while (i < size) {
        const j = this.parseListitem(work, d, o + i, size - i, flags);
        i += j;
        if (!j || flags.v & MKD_LI_END) break;
      }
      html.list(ob, work.s, flags.v);
      return i;
    });
  }

  /** `parse_atxheader`: `#` to `######`. */
  private parseAtxheader(ob: Buf, d: string, o: number, size: number): number {
    let level = 0;
    while (level < size && level < 6 && at(d, o, level) === "#") level++;
    let i: number;
    for (i = level; i < size && at(d, o, i) === " "; i++);
    let end: number;
    for (end = i; end < size && at(d, o, end) !== "\n"; end++);
    const skip = end;
    while (end && at(d, o, end - 1) === "#") end--;
    while (end && at(d, o, end - 1) === " ") end--;
    if (end > i) {
      const e = end;
      this.withBuf((work) => {
        this.parseInline(work, d, o + i, e - i);
        html.header(ob, work.s, level);
      });
    }
    return skip;
  }

  /** `parse_htmlblock`: a block-level tag up to its closing tag and a blank line, an HTML comment, or `<hr>`. */
  private parseHtmlblock(ob: Buf, d: string, o: number, size: number, doRender: boolean): number {
    // identification of the opening tag
    if (size < 2 || at(d, o, 0) !== "<") return 0;
    let i = 1;
    while (i < size && at(d, o, i) !== ">" && at(d, o, i) !== " ") i++;
    let curtag: string | null = null;
    if (i < size) {
      const name = d.slice(o + 1, o + i);
      if (name.length >= 1 && name.length <= 10 && BLOCK_TAGS.has(name.toLowerCase())) curtag = name.toLowerCase();
    }
    // handling of special cases
    if (!curtag) {
      let j = 0;
      // HTML comment, laxist form
      if (size > 5 && at(d, o, 1) === "!" && at(d, o, 2) === "-" && at(d, o, 3) === "-") {
        i = 5;
        while (i < size && !(at(d, o, i - 2) === "-" && at(d, o, i - 1) === "-" && at(d, o, i) === ">")) i++;
        i++;
        if (i < size) j = isEmpty(d, o + i, size - i);
        if (j) {
          if (doRender) html.blockhtml(ob, d.slice(o, o + i + j));
          return i + j;
        }
      }
      // HR, which is the only self-closing block tag considered
      if (size > 4 && (at(d, o, 1) === "h" || at(d, o, 1) === "H") && (at(d, o, 2) === "r" || at(d, o, 2) === "R")) {
        i = 3;
        while (i < size && at(d, o, i) !== ">") i++;
        if (i + 1 < size) {
          i++;
          j = isEmpty(d, o + i, size - i);
          if (j) {
            if (doRender) html.blockhtml(ob, d.slice(o, o + i + j));
            return i + j;
          }
        }
      }
      // no special case recognised
      return 0;
    }
    // looking for an unindented matching closing tag followed by a blank line
    let tagEnd = htmlblockEnd(curtag, d, o, size, true);
    // if not found, trying a second pass looking for indented match but not if tag is "ins" or "del" (following original Markdown.pl)
    if (!tagEnd && curtag !== "ins" && curtag !== "del") tagEnd = htmlblockEnd(curtag, d, o, size, false);
    if (!tagEnd) return 0;
    // the end of the block has been found
    if (doRender) html.blockhtml(ob, d.slice(o, o + tagEnd));
    return tagEnd;
  }

  /** `parse_table_row`. */
  private parseTableRow(ob: Buf, d: string, o: number, size: number, columns: number, colData: number[], headerFlag: number): void {
    this.withBuf((rowWork) => {
      let i = 0;
      if (i < size && at(d, o, i) === "|") i++;
      let col: number;
      for (col = 0; col < columns && i < size; ++col) {
        while (i < size && mdSpace(at(d, o, i))) i++;
        const cellStart = i;
        while (i < size && at(d, o, i) !== "|") i++;
        let cellEnd = i - 1;
        while (cellEnd > cellStart && mdSpace(at(d, o, cellEnd))) cellEnd--;
        const n = 1 + cellEnd - cellStart;
        this.withBuf((cellWork) => {
          this.parseInline(cellWork, d, o + cellStart, n);
          html.tableCell(rowWork, cellWork.s, colData[col]! | headerFlag);
        });
        i++;
      }
      for (; col < columns; ++col) html.tableCell(rowWork, "", colData[col]! | headerFlag);
      html.tableRow(ob, rowWork.s);
    });
  }

  /** `parse_table_header`: the header row and the `|---|:---:|` line under it; 0 when they are not a table. */
  private parseTableHeader(ob: Buf, d: string, o: number, size: number): { end: number; columns: number; colData: number[] } {
    let pipes = 0;
    let i = 0;
    while (i < size && at(d, o, i) !== "\n") if (at(d, o, i++) === "|") pipes++;
    if (i === size || pipes === 0) return { end: 0, columns: 0, colData: [] };
    let headerEnd = i;
    while (headerEnd > 0 && mdSpace(at(d, o, headerEnd - 1))) headerEnd--;
    if (at(d, o, 0) === "|") pipes--;
    if (headerEnd && at(d, o, headerEnd - 1) === "|") pipes--;
    const columns = pipes + 1;
    const colData = new Array<number>(Math.max(columns, 0)).fill(0);
    // Parse the header underline
    i++;
    if (i < size && at(d, o, i) === "|") i++;
    let underEnd = i;
    while (underEnd < size && at(d, o, underEnd) !== "\n") underEnd++;
    let col: number;
    for (col = 0; col < columns && i < underEnd; ++col) {
      let dashes = 0;
      while (i < underEnd && at(d, o, i) === " ") i++;
      if (at(d, o, i) === ":") {
        i++;
        colData[col] = (colData[col] ?? 0) | MKD_TABLE_ALIGN_L;
        dashes++;
      }
      while (i < underEnd && at(d, o, i) === "-") {
        i++;
        dashes++;
      }
      if (i < underEnd && at(d, o, i) === ":") {
        i++;
        colData[col] = (colData[col] ?? 0) | MKD_TABLE_ALIGN_R;
        dashes++;
      }
      while (i < underEnd && at(d, o, i) === " ") i++;
      if (i < underEnd && at(d, o, i) !== "|") break;
      if (dashes < 3) break;
      i++;
    }
    if (col < columns) return { end: 0, columns, colData };
    this.parseTableRow(ob, d, o, headerEnd, columns, colData, MKD_TABLE_HEADER);
    return { end: underEnd + 1, columns, colData };
  }

  /** `parse_table`. */
  private parseTable(ob: Buf, d: string, o: number, size: number): number {
    const headerWork = new Buf();
    const bodyWork = new Buf();
    this.nesting += 2;
    try {
      const h = this.parseTableHeader(headerWork, d, o, size);
      let i = h.end;
      if (i > 0) {
        while (i < size) {
          let pipes = 0;
          const rowStart = i;
          while (i < size && at(d, o, i) !== "\n") if (at(d, o, i++) === "|") pipes++;
          if (pipes === 0 || i === size) {
            i = rowStart;
            break;
          }
          this.parseTableRow(bodyWork, d, o + rowStart, i - rowStart, h.columns, h.colData, 0);
          i++;
        }
        html.table(ob, headerWork.s, bodyWork.s);
      }
      return i;
    } finally {
      this.nesting -= 2;
    }
  }

  /** `parse_block`: the blocks of `d[o .. o+size)`, one after the other. */
  parseBlock(ob: Buf, d: string, o: number, size: number): void {
    let beg = 0;
    if (this.nesting > MAX_NESTING) return;
    while (beg < size) {
      const txt = o + beg;
      const end = size - beg;
      let i: number;
      if (isAtxheader(d, txt)) beg += this.parseAtxheader(ob, d, txt, end);
      else if (at(d, o, beg) === "<" && (i = this.parseHtmlblock(ob, d, txt, end, true)) !== 0) beg += i;
      else if ((i = isEmpty(d, txt, end)) !== 0) beg += i;
      else if (isHrule(d, txt, end)) {
        html.hrule(ob);
        while (beg < size && at(d, o, beg) !== "\n") beg++;
        beg++;
      } else if ((i = this.parseTable(ob, d, txt, end)) !== 0) beg += i;
      else if (prefixQuote(d, txt, end)) beg += this.parseBlockquote(ob, d, txt, end);
      else if (prefixCode(d, txt, end)) beg += this.parseBlockcode(ob, d, txt, end);
      else if (prefixUli(d, txt, end)) beg += this.parseList(ob, d, txt, end, 0);
      else if (prefixOli(d, txt, end)) beg += this.parseList(ob, d, txt, end, MKD_LIST_ORDERED);
      else beg += this.parseParagraph(ob, d, txt, end);
    }
  }

  // ---------------------------------------------------------------------------------------------------------------------- the document

  /** `is_ref`: `[id]: url "title"` on its own line(s), kept in the reference table; where the line ends, or 0 when it is not one. */
  private isRef(data: string, beg: number, end: number): number {
    let i = 0;
    // up to 3 optional leading spaces
    if (beg + 3 >= end) return 0;
    if (data[beg] === " ") {
      i = 1;
      if (data[beg + 1] === " ") {
        i = 2;
        if (data[beg + 2] === " ") {
          i = 3;
          if (data[beg + 3] === " ") return 0;
        }
      }
    }
    i += beg;
    // id part: anything but a newline between brackets
    if (data[i] !== "[") return 0;
    i++;
    const idOffset = i;
    while (i < end && data[i] !== "\n" && data[i] !== "\r" && data[i] !== "]") i++;
    if (i >= end || data[i] !== "]") return 0;
    const idEnd = i;
    // spacer: colon (space | tab)* newline? (space | tab)*
    i++;
    if (i >= end || data[i] !== ":") return 0;
    i++;
    while (i < end && data[i] === " ") i++;
    if (i < end && (data[i] === "\n" || data[i] === "\r")) {
      i++;
      if (i < end && data[i] === "\r" && data[i - 1] === "\n") i++;
    }
    while (i < end && data[i] === " ") i++;
    if (i >= end) return 0;
    // link: whitespace-free sequence, optionally between angle brackets
    if (data[i] === "<") i++;
    const linkOffset = i;
    while (i < end && data[i] !== " " && data[i] !== "\n" && data[i] !== "\r") i++;
    const linkEnd = data[i - 1] === ">" ? i - 1 : i;
    // optional spacer: (space | tab)* (newline | '\'' | '"' | '(' )
    while (i < end && data[i] === " ") i++;
    if (i < end && data[i] !== "\n" && data[i] !== "\r" && data[i] !== "'" && data[i] !== '"' && data[i] !== "(") return 0;
    let lineEnd = 0;
    // computing end-of-line
    if (i >= end || data[i] === "\r" || data[i] === "\n") lineEnd = i;
    if (i + 1 < end && data[i] === "\n" && data[i + 1] === "\r") lineEnd = i + 1;
    // optional (space|tab)* spacer after a newline
    if (lineEnd) {
      i = lineEnd + 1;
      while (i < end && data[i] === " ") i++;
    }
    // optional title: any non-newline sequence enclosed in '"() alone on its line
    let titleOffset = 0;
    let titleEnd = 0;
    if (i + 1 < end && (data[i] === "'" || data[i] === '"' || data[i] === "(")) {
      i++;
      titleOffset = i;
      // looking for EOL
      while (i < end && data[i] !== "\n" && data[i] !== "\r") i++;
      if (i + 1 < end && data[i] === "\n" && data[i + 1] === "\r") titleEnd = i + 1;
      else titleEnd = i;
      // stepping back
      i -= 1;
      while (i > titleOffset && data[i] === " ") i -= 1;
      if (i > titleOffset && (data[i] === "'" || data[i] === '"' || data[i] === ")")) {
        lineEnd = titleEnd;
        titleEnd = i;
      }
    }
    if (!lineEnd || linkEnd === linkOffset) return 0; // garbage after the link empty link
    // a valid ref has been found, filling-in return structures (`add_link_ref` puts it first in its chain: the last one of an id wins)
    this.refs.set(hashLinkRef(data.slice(idOffset, idEnd)), {
      link: data.slice(linkOffset, linkEnd),
      title: titleEnd > titleOffset ? data.slice(titleOffset, titleEnd) : null,
    });
    return lineEnd;
  }

  /** `sd_markdown_render`: references out, tabs expanded and line ends made `\n` in a first pass, then the blocks. */
  render(document: string): string {
    let text = "";
    let beg = 0;
    // Skip a possible UTF-8 BOM, even though the Unicode standard discourages having these in UTF-8 documents
    if (document.startsWith("﻿")) beg = 1;
    const size = document.length;
    while (beg < size) {
      // iterating over lines
      const refEnd = this.isRef(document, beg, size);
      if (refEnd) {
        beg = refEnd;
        continue;
      }
      // skipping to the next line
      let end = beg;
      while (end < size && document[end] !== "\n" && document[end] !== "\r") end++;
      // adding the line body if present
      if (end > beg) text += expandTabs(document.slice(beg, end));
      while (end < size && (document[end] === "\n" || document[end] === "\r")) {
        // add one \n per newline
        if (document[end] === "\n" || (end + 1 < size && document[end + 1] !== "\n")) text += "\n";
        end++;
      }
      beg = end;
    }
    const ob = new Buf();
    if (text.length) {
      // adding a final newline if not already present
      if (text[text.length - 1] !== "\n" && text[text.length - 1] !== "\r") text += "\n";
      this.parseBlock(ob, text, 0, text.length);
    }
    return ob.s;
  }
}

// ---------------------------------------------------------------------------------------------------------------------- block predicates

/** `is_empty`: the length of the line when it holds only spaces, else 0. */
function isEmpty(d: string, o: number, size: number): number {
  let i: number;
  for (i = 0; i < size && at(d, o, i) !== "\n"; i++) if (at(d, o, i) !== " ") return 0;
  return i + 1;
}

/** `is_hrule`: three or more `*`, `-` or `_` alone on the line (spaces between them allowed). */
function isHrule(d: string, o: number, size: number): boolean {
  let i = 0;
  let n = 0;
  if (size < 3) return false;
  if (at(d, o, 0) === " ") {
    i++;
    if (at(d, o, 1) === " ") {
      i++;
      if (at(d, o, 2) === " ") i++;
    }
  }
  if (i + 2 >= size || (at(d, o, i) !== "*" && at(d, o, i) !== "-" && at(d, o, i) !== "_")) return false;
  const c = at(d, o, i);
  while (i < size && at(d, o, i) !== "\n") {
    if (at(d, o, i) === c) n++;
    else if (at(d, o, i) !== " ") return false;
    i++;
  }
  return n >= 3;
}

/** `is_atxheader` (no MKDEXT_SPACE_HEADERS: `#x` is a header too). */
const isAtxheader = (d: string, o: number): boolean => at(d, o, 0) === "#";

/** `is_headerline`: a setext underline, `===` (1) or `---` (2). */
function isHeaderline(d: string, o: number, size: number): number {
  let i = 0;
  if (at(d, o, i) === "=") {
    for (i = 1; i < size && at(d, o, i) === "="; i++);
    while (i < size && at(d, o, i) === " ") i++;
    return i >= size || at(d, o, i) === "\n" ? 1 : 0;
  }
  if (at(d, o, i) === "-") {
    for (i = 1; i < size && at(d, o, i) === "-"; i++);
    while (i < size && at(d, o, i) === " ") i++;
    return i >= size || at(d, o, i) === "\n" ? 2 : 0;
  }
  return 0;
}

function isNextHeaderline(d: string, o: number, size: number): number {
  let i = 0;
  while (i < size && at(d, o, i) !== "\n") i++;
  if (++i >= size) return 0;
  return isHeaderline(d, o + i, size - i);
}

/** `prefix_quote`: up to three spaces, `>` and an optional space. */
function prefixQuote(d: string, o: number, size: number): number {
  let i = 0;
  if (i < size && at(d, o, i) === " ") i++;
  if (i < size && at(d, o, i) === " ") i++;
  if (i < size && at(d, o, i) === " ") i++;
  if (i < size && at(d, o, i) === ">") {
    if (i + 1 < size && at(d, o, i + 1) === " ") return i + 2;
    return i + 1;
  }
  return 0;
}

/** `prefix_code`: four spaces. */
const prefixCode = (d: string, o: number, size: number): number =>
  size > 3 && at(d, o, 0) === " " && at(d, o, 1) === " " && at(d, o, 2) === " " && at(d, o, 3) === " " ? 4 : 0;

/** `prefix_oli`: `1. `. */
function prefixOli(d: string, o: number, size: number): number {
  let i = 0;
  if (i < size && at(d, o, i) === " ") i++;
  if (i < size && at(d, o, i) === " ") i++;
  if (i < size && at(d, o, i) === " ") i++;
  if (i >= size || !isDigit(at(d, o, i))) return 0;
  while (i < size && isDigit(at(d, o, i))) i++;
  if (i + 1 >= size || at(d, o, i) !== "." || at(d, o, i + 1) !== " ") return 0;
  if (isNextHeaderline(d, o + i, size - i)) return 0;
  return i + 2;
}

/** `prefix_uli`: `* `, `+ ` or `- `. */
function prefixUli(d: string, o: number, size: number): number {
  let i = 0;
  if (i < size && at(d, o, i) === " ") i++;
  if (i < size && at(d, o, i) === " ") i++;
  if (i < size && at(d, o, i) === " ") i++;
  if (i + 1 >= size || (at(d, o, i) !== "*" && at(d, o, i) !== "+" && at(d, o, i) !== "-") || at(d, o, i + 1) !== " ") return 0;
  if (isNextHeaderline(d, o + i, size - i)) return 0;
  return i + 2;
}

/** `htmlblock_end_tag`: `</tag>` followed by blank line(s); its length, or 0. */
function htmlblockEndTag(tag: string, d: string, o: number, size: number): number {
  const len = tag.length;
  if (len + 3 >= size || d.slice(o + 2, o + 2 + len).toLowerCase() !== tag || at(d, o, len + 2) !== ">") return 0;
  // checking white lines
  let i = len + 3;
  let w = 0;
  if (i < size && (w = isEmpty(d, o + i, size - i)) === 0) return 0; // non-blank after tag
  i += w;
  w = 0;
  if (i < size) w = isEmpty(d, o + i, size - i);
  return i + w;
}

/** `htmlblock_end`. */
function htmlblockEnd(curtag: string, d: string, o: number, size: number, startOfLine: boolean): number {
  let i = 1;
  let blockLines = 0;
  while (i < size) {
    i++;
    while (i < size && !(at(d, o, i - 1) === "<" && at(d, o, i) === "/")) {
      if (at(d, o, i) === "\n") blockLines++;
      i++;
    }
    // If we are only looking for unindented tags, skip the tag if it doesn't follow a newline. The only exception to this is if the tag is
    // still on the initial line; in that case it still counts as a closing tag
    if (startOfLine && blockLines > 0 && at(d, o, i - 2) !== "\n") continue;
    if (i + 2 + curtag.length >= size) break;
    const endTag = htmlblockEndTag(curtag, d, o + i - 1, size - i + 1);
    if (endTag) return i + endTag - 1;
  }
  return 0;
}

// -------------------------------------------------------------------------------------------------------------------------- inline helpers

/** `is_mail_autolink`: `[-@._a-zA-Z0-9]+` with exactly one `@`, then `>`. */
function isMailAutolink(d: string, o: number, size: number): number {
  let nb = 0;
  for (let i = 0; i < size; ++i) {
    const c = at(d, o, i);
    if (cAlnum(c)) continue;
    switch (c) {
      case "@":
        nb++;
        break;
      case "-":
      case ".":
      case "_":
        break;
      case ">":
        return nb === 1 ? i + 1 : 0;
      default:
        return 0;
    }
  }
  return 0;
}

/** `tag_length`: how long the tag (or `<scheme:...>` / `<a@b>` autolink) at `d[o]` is, 0 when it is not one. */
function tagLength(d: string, o: number, size: number): { end: number; autolink: Autolink } {
  const none = { end: 0, autolink: Autolink.NotAutolink };
  // a valid tag can't be shorter than 3 chars
  if (size < 3) return none;
  // begins with a '<' optionally followed by '/', followed by letter or number
  if (at(d, o, 0) !== "<") return none;
  let i = at(d, o, 1) === "/" ? 2 : 1;
  if (!cAlnum(at(d, o, i))) return none;
  // scheme test
  let autolink = Autolink.NotAutolink;
  // try to find the beginning of an URI
  while (i < size && (cAlnum(at(d, o, i)) || at(d, o, i) === "." || at(d, o, i) === "+" || at(d, o, i) === "-")) i++;
  if (i > 1 && at(d, o, i) === "@") {
    const j = isMailAutolink(d, o + i, size - i);
    if (j !== 0) return { end: i + j, autolink: Autolink.Email };
  }
  if (i > 2 && at(d, o, i) === ":") {
    autolink = Autolink.Normal;
    i++;
  }
  // completing autolink test: no whitespace or ' or "
  if (i >= size) autolink = Autolink.NotAutolink;
  else if (autolink !== Autolink.NotAutolink) {
    const j = i;
    while (i < size) {
      if (at(d, o, i) === "\\") i += 2;
      else if (at(d, o, i) === ">" || at(d, o, i) === "'" || at(d, o, i) === '"' || at(d, o, i) === " " || at(d, o, i) === "\n") break;
      else i++;
    }
    if (i >= size) return none;
    if (i > j && at(d, o, i) === ">") return { end: i + 1, autolink };
    // one of the forbidden chars has been found
    autolink = Autolink.NotAutolink;
  }
  // looking for something looking like a tag end
  while (i < size && at(d, o, i) !== ">") i++;
  if (i >= size) return none;
  return { end: i + 1, autolink };
}

/** `unscape_text`: a backslash keeps the character after it. */
function unescapeText(src: string): string {
  let out = "";
  let i = 0;
  while (i < src.length) {
    const org = i;
    while (i < src.length && src[i] !== "\\") i++;
    if (i > org) out += src.slice(org, i);
    if (i + 1 >= src.length) break;
    out += src[i + 1];
    i += 2;
  }
  return out;
}

/** `hash_link_ref`: the reference table's key (case-blind). */
function hashLinkRef(id: string): number {
  let hash = 0;
  for (let i = 0; i < id.length; ++i) {
    const c = id.charCodeAt(i);
    const lower = c >= 65 && c <= 90 ? c + 32 : c; // C `tolower`
    hash = (lower + (hash << 6) + (hash << 16) - hash) >>> 0;
  }
  return hash;
}

/** `expand_tabs`: a tab to the next multiple of four. */
function expandTabs(line: string): string {
  let out = "";
  let tab = 0;
  for (const ch of line) {
    if (ch !== "\t") {
      out += ch;
      tab++;
      continue;
    }
    do {
      out += " ";
      tab++;
    } while (tab % 4);
  }
  return out;
}

/** `ConvertMarkdown2Html( aMarkdownInput, aHtmlOutput )`: KiCad's markdown, as the HTML its help windows show. */
export function convertMarkdown2Html(markdown: string): string {
  return new Markdown().render(markdown);
}
