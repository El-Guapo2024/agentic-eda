import { test } from "node:test";
import assert from "node:assert/strict";
import { cropViewBox, fluidSvg, sanitizeSvg } from "./svgPreview";

const KICAD_SVG = `<?xml version="1.0" standalone="no"?>
 <!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" 
 "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd"> 
<svg xmlns="http://www.w3.org/2000/svg" version="1.1" width="297.0022mm" height="210.0072mm" viewBox="0.0000 0.0000 297.0022 210.0072">
<title>SVG Image created as x.svg</title>
<g style="fill:#000000;"><path d="M 33.0 170.0 L 40 40" style="stroke:#840000"/><text x="1" y="2">R1</text></g>
</svg>`;

test("what kicad-cli writes keeps its drawing and loses its prolog and doctype", () => {
  const s = sanitizeSvg(KICAD_SVG);
  assert.ok(s.startsWith("<svg"));
  assert.ok(s.includes('<path d="M 33.0 170.0 L 40 40"') && s.includes(">R1</text>"));
  assert.ok(!s.includes("<?xml") && !s.includes("DOCTYPE"));
});

test("a script, a foreign object, an event handler and a javascript link are taken out", () => {
  const dirty = `<svg onload="evil()" width="1mm"><script>alert(1)</script><g onclick='x()'><foreignObject><div>hi</div></foreignObject><a href="javascript:alert(2)"><path d="M0 0"/></a><image href="http://x/y.png"/><use xlink:href="#a"/></g></svg>`;
  const s = sanitizeSvg(dirty);
  for (const bad of ["onload", "onclick", "<script", "alert", "foreignObject", "javascript:", "<image", "<use"]) assert.ok(!s.includes(bad), `${bad} in ${s}`);
  assert.ok(s.includes('<path d="M0 0"/>'));
});

test("the picture is made fluid by its root alone", () => {
  const fluid = fluidSvg(sanitizeSvg(KICAD_SVG));
  assert.ok(fluid.startsWith('<svg xmlns="http://www.w3.org/2000/svg" version="1.1" width="100%" height="100%" viewBox='));
  assert.equal(fluidSvg("<g/>"), "<g/>");
});

test("a drawing's box gets a margin and nothing else is cropped to", () => {
  assert.equal(cropViewBox({ x: 10, y: 20, width: 100, height: 50 }), "0 10 120 70");
  assert.equal(cropViewBox({ x: 0, y: 0, width: 0, height: 5 }), null);
  assert.equal(cropViewBox({ x: NaN, y: 0, width: 5, height: 5 }), null);
});
