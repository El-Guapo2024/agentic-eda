import { test } from "node:test";
import assert from "node:assert/strict";
import { footprintDocumentationUrl } from "./footprintDatasheet";

const field = (name: string, value: string) => ({ name, value, visible: false });

test("the Datasheet field is the documentation when it has text", () => {
  assert.equal(footprintDocumentationUrl([field("Datasheet", "https://example.com/a.pdf")], "see https://other.example/b"), "https://example.com/a.pdf");
  assert.equal(footprintDocumentationUrl([field("datasheet", "  www.example.com/a.pdf ")], ""), "www.example.com/a.pdf");
});

test("an empty Datasheet field falls back to the first address in the description", () => {
  assert.equal(footprintDocumentationUrl([field("Datasheet", "")], "Resistor, see https://example.com/r.pdf for the body"), "https://example.com/r.pdf");
  assert.equal(footprintDocumentationUrl([], "http://example.com/x"), "http://example.com/x");
  assert.equal(footprintDocumentationUrl([field("Vendor", "ACME")], "plain text"), null);
  assert.equal(footprintDocumentationUrl([], ""), null);
});

test("the address ends at a character a URI cannot hold, or at the bracket that closes an open one", () => {
  assert.equal(footprintDocumentationUrl([], 'link "https://example.com/a" more'), "https://example.com/a");
  assert.equal(footprintDocumentationUrl([], "(Body style from: https://example.com/part.pdf) and so on"), "https://example.com/part.pdf");
  assert.equal(footprintDocumentationUrl([], "https://example.com/a_(b)_c ok"), "https://example.com/a_(b)_c");
  assert.equal(footprintDocumentationUrl([], "https://example.com/café"), "https://example.com/caf");
});

test("trailing punctuation is dropped, once", () => {
  assert.equal(footprintDocumentationUrl([], "See https://example.com/a.pdf."), "https://example.com/a.pdf");
  assert.equal(footprintDocumentationUrl([], "See https://example.com/a.pdf,"), "https://example.com/a.pdf");
  assert.equal(footprintDocumentationUrl([], "See https://example.com/a.pdf.."), "https://example.com/a.pdf.");
});

test("http: is looked for before https:, as the source does", () => {
  assert.equal(footprintDocumentationUrl([], "https://b.example/x and later http://a.example/y"), "http://a.example/y");
});
