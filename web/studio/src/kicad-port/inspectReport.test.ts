import { test } from "node:test";
import assert from "node:assert/strict";
import type { InspectPage, InspectReply } from "../api/types";
import { decodeEntities, pageLines, parseReportHtml, reportPlainText, reportText } from "./inspectReport";

test("EscapeHTML's five entities and &nbsp; read back, numeric references too", () => {
  assert.equal(decodeEntities("A.NetName == &apos;GND&apos; &amp;&amp; B &lt; 1 &gt; &quot;x&quot;"), "A.NetName == 'GND' && B < 1 > \"x\"");
  assert.equal(decodeEntities("a&nbsp;&nbsp;b"), "a  b");
  assert.equal(decodeEntities("&#65;&#x42;&unknown;"), "AB&unknown;");
});

test("a report header: <h7> and a list, with the text unescaped", () => {
  const h = parseReportHtml("<h7>Clearance resolution for:</h7>");
  assert.deepEqual(h, [{ tag: "h7", href: undefined, children: ["Clearance resolution for:"] }]);
  const l = parseReportHtml("<ul><li>Layer F.Cu</li><li>Pad 1 [GND] of R1 on F.Cu [netclass Default]</li></ul>");
  assert.equal(l.length, 1);
  assert.equal(reportText(l), "Layer F.CuPad 1 [GND] of R1 on F.Cu [netclass Default]");
});

test("bold, italics and the Show design rules link", () => {
  const nodes = parseReportHtml("Item <b>disallowed</b> at current location. <i>undefined</i>");
  assert.equal(reportText(nodes), "Item disallowed at current location. undefined");
  assert.deepEqual(nodes[1], { tag: "b", href: undefined, children: ["disallowed"] });
  const link = parseReportHtml("Report incomplete.&nbsp;&nbsp;<a href='$CUSTOM_RULES'>Show design rules.</a>");
  assert.deepEqual(link[link.length - 1], { tag: "a", href: "$CUSTOM_RULES", children: ["Show design rules."] });
});

test("a tag that is not in the list is dropped and never becomes markup, an unclosed one is closed", () => {
  const nodes = parseReportHtml("<script>alert(1)</script><b>bold");
  assert.equal(reportText(nodes), "alert(1)bold");
  assert.ok(nodes.every((n) => typeof n === "string" || n.tag === "b"));
  assert.equal(reportText(parseReportHtml("1 < 2")), "1 < 2");
  assert.equal(reportText(parseReportHtml("<img src=x onerror=alert(1)>text")), "text");
});

test("a page with a layer choice shows the lines of the layer picked, a plain page its own", () => {
  const page: InspectPage = { title: "Clearance", lines: [], per_layer: [{ layer: "F.Cu", name: "F.Cu", lines: ["front"] }, { layer: "B.Cu", name: "B.Cu", lines: ["back"] }], resolutions: [] };
  assert.deepEqual(pageLines(page, 0), ["front"]);
  assert.deepEqual(pageLines(page, 1), ["back"]);
  assert.deepEqual(pageLines(page, 7), ["back"], "clamped");
  assert.deepEqual(pageLines({ ...page, per_layer: [], lines: ["own"] }, 3), ["own"]);
});

test("the whole report as text, page by page", () => {
  const reply: InspectReply = { ok: true, title: "Clearance Report", compile_error: null, pages: [{ title: "F.Cu", lines: ["<h7>Clearance resolution for:</h7>", "Resolved min clearance: 0.2 mm."], per_layer: [], resolutions: [] }] };
  assert.equal(reportPlainText(reply), "== F.Cu ==\nClearance resolution for:\nResolved min clearance: 0.2 mm.");
});
