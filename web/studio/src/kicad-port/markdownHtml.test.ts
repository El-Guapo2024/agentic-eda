import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { convertMarkdown2Html } from "./markdownHtml";
import { syntaxHelpHtml, syntaxHelpMarkdown } from "./rulesSyntaxHelp";

type Help = { texts: Record<string, string>; setupRules: string[]; ruleEditor: string[] };
type Golden = { cases: { name: string; markdown?: string; help?: "setupRules" | "ruleEditor"; html: string }[] };

const help = JSON.parse(readFileSync("src/kicad/rules_help.json", "utf8")) as Help;
const golden = JSON.parse(readFileSync("src/kicad-port/markdownHtml.golden.json", "utf8")) as Golden;
const texts = (key: "setupRules" | "ruleEditor") => help[key].map((f) => help.texts[f]!);

test("ConvertMarkdown2Html: what KiCad's own sundown makes of the help texts and of every sample (markdownHtml.golden.json), byte for byte", () => {
  assert.ok(golden.cases.length >= 30, `${golden.cases.length} cases`);
  for (const c of golden.cases) {
    const md = c.help ? texts(c.help).join("") : c.markdown!;
    assert.equal(convertMarkdown2Html(md), c.html, c.name);
  }
});

/** `###` headings, `|---|` table underlines, runs of 4-space-indented lines and ``` pairs, read off the markdown independently of the port. */
function sourceParts(md: string) {
  const lines = md.split("\n");
  const headings = lines.filter((l) => /^### /.test(l)).map((l) => l.slice(4).trim());
  const tables = lines.filter((l) => /^\|[-| :]+\|\s*$/.test(l)).length;
  const blocks: string[] = [];
  for (let i = 0; i < lines.length; i++) {
    if (/^ {4}\S/.test(lines[i]!) && (i === 0 || lines[i - 1]!.trim() === "")) {
      // a code block starts after a blank line; it runs over blank lines while the next text line is indented
      blocks.push(lines[i]!.slice(4));
      while (i + 1 < lines.length && (/^ {4}/.test(lines[i + 1]!) || (lines[i + 1]!.trim() === "" && lines.slice(i + 1).find((l) => l.trim() !== "")?.startsWith("    ")))) i++;
    }
  }
  const fences = lines.filter((l) => l.trim() === "```").length / 2;
  return { headings, tables, blocks, fences };
}

const escape = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#39;");

test("the Custom Rules help as HTML has every heading, table and code block of KiCad's markdown", () => {
  for (const key of ["setupRules", "ruleEditor"] as const) {
    const md = syntaxHelpMarkdown(texts(key), false);
    const html = syntaxHelpHtml(texts(key), false);
    const src = sourceParts(md);
    const h3 = [...html.matchAll(/<h3>(.*?)<\/h3>/g)].map((m) => m[1]);
    assert.deepEqual(h3, src.headings, `${key}: the headings`);
    assert.equal((html.match(/<table>/g) ?? []).length, src.tables, `${key}: the tables`);
    const pres = [...html.matchAll(/<pre><code>([\s\S]*?)<\/code><\/pre>/g)].map((m) => m[1]!);
    assert.equal(pres.length, src.blocks.length, `${key}: the indented code blocks`);
    src.blocks.forEach((first, i) => assert.ok(pres[i]!.startsWith(escape(first)), `${key}: code block ${i} starts with ${first}`));
    // KiCad's ``` lines are not fences (MKDEXT_FENCED_CODE is off): each pair is a code span in a paragraph.
    assert.equal((html.match(/<p><code>\n/g) ?? []).length, src.fences, `${key}: the \`\`\` spans`);
  }
  const html = syntaxHelpHtml(texts("setupRules"), false);
  assert.ok(html.includes("<h3>Top-level Clauses</h3>") && html.includes("<h3>Expression functions</h3>") && html.includes("<h3>Documentation</h3>"));
  assert.equal((html.match(/<tr>/g) ?? []).length, 1 + 34, "the constraints table: its header and 34 constraints");
  assert.ok(html.includes('<a href="https://go.kicad.org/docs/GetMajorMinorVersion/en/pcbnew/pcbnew.html#custom_design_rules">https://docs.kicad.org</a>'));
});

test("OnSyntaxHelp: every table cell is valign=top, Cmd+ for Ctrl+ on a Mac only", () => {
  const html = syntaxHelpHtml(texts("setupRules"), false);
  assert.ok(!/<td(?! valign=top)/.test(html), "every <td is <td valign=top");
  assert.ok((html.match(/<td valign=top>/g) ?? []).length > 100);
  assert.ok(html.includes("Use Ctrl+/ to comment or uncomment line(s)."));
  const mac = syntaxHelpHtml(texts("setupRules"), true);
  assert.ok(mac.includes("Use Cmd+/ to comment or uncomment line(s).") && !mac.includes("Ctrl+"));
});

test("the help windows show KiCad's texts in OnSyntaxHelp's order (rules_help.json)", () => {
  assert.deepEqual(
    help.setupRules.map((f) => f.replace(/^pcbnew\/dialogs\/panel_setup_rules_help_|\.h$/g, "")),
    ["1clauses", "2constraints", "3items", "4severity_names", "5examples", "6notes", "7properties", "8expression_functions", "9more_examples", "10documentation"]
  );
  assert.equal(help.ruleEditor[0], "pcbnew/dialogs/panel_setup_condition_help_1clauses.h");
  assert.equal(help.ruleEditor.length, 8);
  assert.ok(help.texts["pcbnew/dialogs/panel_setup_rules_help_1clauses.h"]!.startsWith("### Top-level Clauses\n\n    (version 1)\n"));
});

test("sundown corners: a lone & and < are escaped, entities pass, raw tags pass, tabs go to 4 columns", () => {
  assert.equal(convertMarkdown2Html("a & b < c &amp; <b>x</b>"), "<p>a &amp; b &lt; c &amp; <b>x</b></p>\n");
  assert.equal(convertMarkdown2Html("\tcode"), "<pre><code>code\n</code></pre>\n");
  assert.equal(convertMarkdown2Html("x\ty"), "<p>x   y</p>\n");
  assert.equal(convertMarkdown2Html(""), "");
});
