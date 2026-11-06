import { test } from "node:test";
import assert from "node:assert/strict";
import { htmlLinkHref, nextMatch, prevMatch, searchHtml } from "./htmlMessageBox";

test("OnHTMLLinkClicked: a go.kicad.org/docs link gets the version, others stay", () => {
  assert.equal(htmlLinkHref("https://go.kicad.org/docs/GetMajorMinorVersion/en/pcbnew/pcbnew.html#custom_design_rules", "10.99"), "https://go.kicad.org/docs/10.99/en/pcbnew/pcbnew.html#custom_design_rules");
  assert.equal(htmlLinkHref("https://example.com/GetMajorMinorVersion", "10.99"), "https://example.com/GetMajorMinorVersion");
});

test("updateSearch: matches in the text, not in tags; case ignored; the current one violet after its anchor", () => {
  const src = '<p class="clearance">Clearance and clearance</p>';
  const r = searchHtml(src, "clearance", 0);
  assert.deepEqual(r.matches, [21, 35]);
  assert.equal(r.label, "1/2");
  assert.equal(r.anchor, "kicad_search_0");
  assert.equal(
    r.html,
    '<p class="clearance"><a name="kicad_search_0"></a><span style="background-color:#DDAAFF;">Clearance</span> and <span style="background-color:#FFFFAA;">clearance</span></p>'
  );
  const second = searchHtml(src, "clearance", 1);
  assert.equal(second.label, "2/2");
  assert.ok(second.html.includes('<a name="kicad_search_1"></a><span style="background-color:#DDAAFF;">clearance</span>'));
  // past the last match the search goes back to the first
  assert.equal(searchHtml(src, "clearance", 5).current, 0);
});

test("updateSearch: no term, no match, an entity is matched as written, a match into a tag is not one", () => {
  assert.deepEqual(searchHtml("<p>x</p>", "", 3), { html: "<p>x</p>", matches: [], current: 0, label: "", anchor: null });
  const none = searchHtml("<p>x</p>", "zz", 0);
  assert.equal(none.label, "0/0");
  assert.equal(none.html, "<p>x</p>");
  assert.equal(searchHtml("<p>a &lt;b&gt;</p>", "<b", 0).matches.length, 0);
  assert.equal(searchHtml("<p>a &lt;b&gt;</p>", "&lt;b", 0).matches.length, 1);
  assert.ok(searchHtml("<p>a &lt;b&gt;</p>", "&lt;b", 0).html.includes(">&amp;lt;b</span>"), "the match is escaped");
  assert.equal(searchHtml("<p>ab</p><i>c</i>", "b<", 0).matches.length, 0);
});

test("OnNext / OnPrev go round", () => {
  assert.equal(nextMatch(1, 2), 0);
  assert.equal(nextMatch(0, 2), 1);
  assert.equal(prevMatch(0, 3), 2);
  assert.equal(nextMatch(0, 0), 0);
});
