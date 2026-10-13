// What the Clearance and Constraints Resolution dialogs (`DIALOG_BOOK_REPORTER`, pcbnew/dialogs/dialog_book_reporter.cpp) show: a notebook of pages, each the lines of a
// `WX_HTML_REPORT_BOX` (common/widgets/wx_html_report_box.cpp). The lines are HTML as `BOARD_INSPECTION_TOOL` writes them (`<h7>` titles, `<ul><li>` lists, `<b>`, `<i>`, a link,
// entities from `EscapeHTML`); the server (crates/drc/src/inspect.rs) makes them, this file reads them without ever handing the text to the DOM as markup.
import type { InspectPage, InspectReply } from "../api/types";

/** The inline HTML of a report line, as a tree of the few tags KiCad's reports use. */
export type HtmlNode = string | { tag: "h7" | "ul" | "li" | "b" | "i" | "a" | "br"; href?: string; children: HtmlNode[] };

const ENTITIES: Record<string, string> = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " " };

/** `&amp;` and its kin (the five `EscapeHTML` writes, and `&nbsp;`) and numeric references. */
export function decodeEntities(text: string): string {
  return text.replace(/&(#x[0-9a-f]+|#[0-9]+|[a-z]+);/gi, (whole, name: string) => {
    if (name[0] === "#") {
      const code = name[1] === "x" || name[1] === "X" ? parseInt(name.slice(2), 16) : parseInt(name.slice(1), 10);
      return Number.isFinite(code) && code > 0 && code < 0x110000 ? String.fromCodePoint(code) : whole;
    }
    return ENTITIES[name.toLowerCase()] ?? whole;
  });
}

type Tag = Extract<HtmlNode, object>["tag"];
const TAGS = new Set<string>(["h7", "ul", "li", "b", "i", "a", "br"]);

/** A report line as nodes. A tag outside the list is dropped (its text stays), an unclosed one is closed at the end of the line. */
export function parseReportHtml(html: string): HtmlNode[] {
  const root: HtmlNode[] = [];
  const stack: { tag: Tag; children: HtmlNode[]; href?: string }[] = [];
  const out = (): HtmlNode[] => (stack.length > 0 ? stack[stack.length - 1]!.children : root);
  const re = /<(\/?)([a-z0-9]+)([^>]*)>|([^<]+)|</gi;
  let m: RegExpExecArray | null;
  while ((m = re.exec(html)) !== null) {
    if (m[4] !== undefined) {
      out().push(decodeEntities(m[4]));
      continue;
    }
    if (m[2] === undefined) {
      out().push("<");
      continue;
    }
    const tag = m[2].toLowerCase();
    if (!TAGS.has(tag)) continue;
    if (m[1] === "/") {
      // close the nearest open tag of that name (and what was left open inside it)
      const at = stack.map((s) => s.tag).lastIndexOf(tag as Tag);
      if (at < 0) continue;
      while (stack.length > at) {
        const done = stack.pop()!;
        out().push({ tag: done.tag, href: done.href, children: done.children });
      }
      continue;
    }
    if (tag === "br") {
      out().push({ tag: "br", children: [] });
      continue;
    }
    const href = tag === "a" ? /href\s*=\s*(?:'([^']*)'|"([^"]*)")/i.exec(m[3] ?? "") : null;
    stack.push({ tag: tag as Tag, children: [], href: href ? (href[1] ?? href[2]) : undefined });
  }
  while (stack.length > 0) {
    const done = stack.pop()!;
    out().push({ tag: done.tag, href: done.href, children: done.children });
  }
  return root;
}

/** The text of a node list, tags dropped (what a test or a copy to the clipboard wants). */
export function reportText(nodes: readonly HtmlNode[]): string {
  return nodes.map((n) => (typeof n === "string" ? n : n.tag === "br" ? "\n" : reportText(n.children))).join("");
}

/** The lines a page shows: its own, or those of the layer picked in the "Layer:" choice of a page that has one per copper layer. */
export function pageLines(page: InspectPage, layerIndex: number): string[] {
  if (page.per_layer.length === 0) return page.lines;
  return page.per_layer[Math.min(Math.max(layerIndex, 0), page.per_layer.length - 1)]!.lines;
}

/** Every line of a report as plain text, page by page (the layer pages one after the other). */
export function reportPlainText(reply: InspectReply): string {
  return reply.pages
    .map((p) => {
      const sets = p.per_layer.length > 0 ? p.per_layer.map((l) => l.lines) : [p.lines];
      return `== ${p.title} ==\n` + sets.map((lines) => lines.map((l) => reportText(parseReportHtml(l))).join("\n")).join("\n");
    })
    .join("\n\n");
}

/** `DIALOG_BOOK_REPORTER` titles. */
export const CLEARANCE_TITLE = "Clearance Report";
export const CONSTRAINTS_TITLE = "Constraints Report";
