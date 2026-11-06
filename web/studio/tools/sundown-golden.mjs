#!/usr/bin/env node
// What KiCad's own markdown converter makes of the Syntax Help texts and of a set of samples that touch each construct it knows, for
// src/kicad-port/markdownHtml.test.ts to hold the TypeScript port (src/kicad-port/markdownHtml.ts) to, byte for byte. It compiles KiCad's
// bundled sundown (thirdparty/markdown2html: md_parser/*.c and html_formatter/*.c) with tools/lib/sundown_main.c -- `ConvertMarkdown2Html`
// with KiCad's options -- and runs it on each input:
//
//   SUNDOWN_DIR=~/src/kicad-src-25ec2335/thirdparty/markdown2html node tools/sundown-golden.mjs
//
// (8303b2ad's tree on this box has no thirdparty/; git shows thirdparty/markdown2html unchanged from 8303b2ad to 25ec2335.) Needs a C
// compiler (`cc`). Writes src/kicad-port/markdownHtml.golden.json: { sundown, cases: [{ name, markdown | help, html }] }; a `help` case is a
// window of src/kicad/rules_help.json (its texts concatenated as `OnSyntaxHelp` does) so the texts are not stored twice.

import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const OUT = join(here, "..", "src", "kicad-port", "markdownHtml.golden.json");
const HELP = join(here, "..", "src", "kicad", "rules_help.json");

/** Markdown that walks each construct of the parser and the renderer (and their corners). */
export const SAMPLES = [
  ["emphasis", "*a* _b_ **c** __d__ ***e*** ___f___ a*b*c *not closed\n* space after* and **bold *nested* bold** _a_b_ *a **b** c*\n"],
  ["emphasis corners", "**\n\n__x __ y__\n\n*x*y* `*code*` [*l*](u) ***a** b* ***a* b**\n"],
  ["code spans", "`x` ``a ` b`` ` padded ` `` and ``` multi\nline ```\n`unclosed\n"],
  ["escapes", "\\* \\_ \\\\ \\` \\# \\q \\< \\& \\| end\\\n"],
  ["entities", "&amp; &lt; &#39; &nbsp; & lone; &#x41; &copy &;\n"],
  ["inline links", '[text](http://a.b/c?d=e&f=g "Title") [t](<url>) [t]( url \'ti tle\' ) [e]() [*em*](u) [a\n\n[x] (y) [sp]( http://x/a b )\n'],
  ["reference links", '[t][id] and [id] and [Id][] and [t][nope]\n\n[id]: http://x.y/z "The title"\n  [other]: <http://o> (paren title)\n[x]:\n'],
  ["images", '![alt](img.png "t") ![](e.png) ![a *b*](c.png) !not [x](y)\n'],
  ["autolinks and tags", "<http://example.com/a> <https://x.y/z?a=1&b='2'> <a@b.c> <mailto:a@b.c> <http://a b> <b>bold</b> <br> <span class=\"x\"> <1> <>\n"],
  ["block html", "<div>\nhello *x*\n</div>\n\npara\n\n<!-- comment -->\n\n<hr>\n\n<hr/>\n\n<table><tr><td>x</td></tr></table>\n\n<DIV class=a>\n  <p>in</p>\n  </DIV>\n\nafter\n"],
  ["headers", "# H1\n## H2 ##\n#NoSpace\n####### seven\n### trailing ###   \n#\n"],
  ["setext headers", "Title\n=====\n\nSub\n---\n\nline one\nline two\n===\n"],
  ["unordered lists", "* a\n* b\n\n* c\n\n+ plus\n- minus\n\n* item\n  continued\n* item **bold**\n"],
  ["ordered lists", "1. one\n2. two\n10. ten\n\n1. x\n\n   para in item\n2. y\n"],
  ["nested lists", "- a\n    - nested\n    - nested 2\n- b\n  1. sub ordered\n\n* outer\n\n  * inner block\n"],
  ["list switches", "* a\n\n1. b\n\n- c\n- ---\n"],
  ["blockquotes", "> a\n> b\n\n> c\nlazy\n\nd\n\n> * item\n> * item2\n\n> 1. x\n>    continued\n>\n> > nested\n"],
  ["code blocks", "    code <tags> & \"q\"\n\n    more\n\n\n    end\nafter\n\n\tTabbed code\n"],
  ["tables", "| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n| only one |\n| x | y | z | extra |\n\nh1 | h2\n---|---\n1 | `2`\n\n| no | under\n| --- | -- |\n\n|x|\n|---|\nnot a row\n"],
  ["table cells", "| `a|b` | **x** | <br> |\n|---|---|---|\n|  spaced  ||  |\n"],
  ["hrules", "***\n\n- - -\n\n___\n\n  * * *\n\n--\n"],
  ["linebreaks", "line one  \nline two   \nline three\n"],
  ["tabs", "\tindented\na\tb\tc\n\t\tdouble\n"],
  ["line ends", "a\r\nb\r\n\r\nc\rd\n\n\ne"],
  ["bom", "﻿# title\n"],
  ["fence-like paragraphs", "```\n    A.enclosedByArea('<zone_name>')\n```\n> True if all of `A` lies within the given zone's outline.\n\n```\nAB.isCoupledDiffPair()\n```\n"],
  ["mixed", "Use Ctrl+/ to comment.  It should be\nset to \"1\".\n<br><br><br>\n\n### Documentation\n\nFor the full documentation see [https://docs.kicad.org](https://go.kicad.org/docs/GetMajorMinorVersion/en/pcbnew/pcbnew.html#custom_design_rules)."],
  ["empty", ""],
  ["only spaces", "   \n  \n"],
];

function main() {
  const dir = process.env.SUNDOWN_DIR;
  if (!dir) {
    console.error("set SUNDOWN_DIR to KiCad's thirdparty/markdown2html");
    process.exit(2);
  }
  const work = mkdtempSync(join(tmpdir(), "sundown-"));
  try {
    const exe = join(work, "sundown");
    const md = join(dir, "md_parser");
    const fmt = join(dir, "html_formatter");
    execFileSync("cc", ["-O1", "-w", `-I${md}`, `-I${fmt}`, "-o", exe, join(here, "lib", "sundown_main.c"), ...["markdown.c", "buffer.c", "stack.c", "autolink.c"].map((f) => join(md, f)), ...["html.c", "houdini_html_e.c", "houdini_href_e.c"].map((f) => join(fmt, f))]);
    const run = (input) => execFileSync(exe, [], { input: Buffer.from(input, "utf8") }).toString("utf8");
    const help = JSON.parse(readFileSync(HELP, "utf8"));
    const cases = [];
    for (const key of ["setupRules", "ruleEditor"]) {
      // OnSyntaxHelp: `msg_txt << wxGetTranslation( i )` for each text, then ConvertMarkdown2Html (the `<td` replacement comes after it).
      cases.push({ name: `Syntax Help: ${key}`, help: key, html: run(help[key].map((f) => help.texts[f]).join("")) });
    }
    for (const [name, markdown] of SAMPLES) cases.push({ name, markdown, html: run(markdown) });
    const json = JSON.stringify({ sundown: "KiCad thirdparty/markdown2html (sundown 1.16) built by tools/sundown-golden.mjs; markdown2html.cpp's options", helpCommit: help.commit, cases }, null, 2) + "\n";
    writeFileSync(OUT, json);
    console.log(`${cases.length} cases -> ${OUT}`);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
