import { test } from "node:test";
import assert from "node:assert/strict";
import { boardMaterial3d, copperLayerMaterial3d, copperMaterial3d, glShininess, innerLayersDrawn, mapf, maskMaterial3d, pasteMaterial3d, plasticMaterial3d, silkMaterial3d, transparencyControl } from "./material3d";

const near = (a: number, b: number, msg?: string) => assert.ok(Math.abs(a - b) < 1e-6, `${msg ?? ""} ${a} vs ${b}`);

test("setupMaterials' copper: ambient a tenth of the colour, specular three quarters plus a quarter, shininess from how gold it is", () => {
  // The theme's copper (179, 156, 0): |r - g| = 0.09, below mapf's 0.15, so the factor is 0.4 (a "copper" look), 51.2.
  const c = { r: 179 / 255, g: 156 / 255, b: 0 };
  const m = copperMaterial3d(c);
  near(m.ambient.r, c.r * 0.1);
  near(m.specular.g, c.g * 0.75 + 0.25);
  near(m.specular.b, 0.25);
  near(m.shininess, 0.4 * 128);
  // A red copper (|r - g| = 0.6): 0.4 - mapf( 0.6, 0.15, 1, 0, 0.3 ) = 0.4 - 0.1588.
  near(copperMaterial3d({ r: 0.8, g: 0.2, b: 0.2 }).shininess, (0.4 - (0.45 * 0.3) / 0.85) * 128);
  near(mapf(2, 0, 1, 0, 10), 10, "mapf clamps");
  // With the PCB editor's colour: its diffuse that colour, the rest the copper colour's.
  const e = copperLayerMaterial3d(c, { r: 0.5, g: 1, b: 0.5 });
  assert.deepEqual(e.diffuse, { r: 0.5, g: 1, b: 0.5 });
  assert.deepEqual(e.specular, m.specular);
});

test("OglSetMaterial gives OpenGL 128 * min( shininess, 1 ): every board layer is 128", () => {
  for (const m of [copperMaterial3d({ r: 0.7, g: 0.6, b: 0 }), pasteMaterial3d({ r: 0.5, g: 0.5, b: 0.5 }), silkMaterial3d({ r: 0.9, g: 0.9, b: 0.9 }), boardMaterial3d({ r: 0.2, g: 0.17, b: 0.09, a: 0.9 }), plasticMaterial3d({ r: 1, g: 0, b: 0 })]) {
    assert.equal(glShininess(m), 128);
  }
  assert.equal(glShininess({ shininess: 0.5 }), 64);
});

test("the solder mask: TransparencyControl of its colour, a darker one more opaque and shinier, a specular floor of 0.3", () => {
  near(transparencyControl(0, 0), 0);
  near(transparencyControl(0.5, 1), 1);
  // The theme's green, 83 % opaque: gray = (20 + 51 + 36) / 3 / 255.
  const green = { r: 20 / 255, g: 51 / 255, b: 36 / 255, a: 0.83 };
  const m = maskMaterial3d(green);
  const gray = (20 + 51 + 36) / 3 / 255;
  const t = 0.17;
  near(m.transparency, gray * (1 - 1.05 * (1 - t) ** 3) + t ** 3);
  near(m.ambient.g, (51 / 255) * 0.3);
  assert.deepEqual(m.specular, { r: 0.3, g: 0.3, b: 0.3 }, "the floor");
  near(m.shininess, 0.85 * 128 + (512 - 0.85 * 128) * (1 - gray));
  // White mask: brighter specular than the floor, the least shiny.
  const white = maskMaterial3d({ r: 0.96, g: 0.96, b: 0.96, a: 0.83 });
  near(white.specular.r, 0.96 * 0.96);
  assert.ok(white.shininess < m.shininess);
});

test("the paste, the silkscreen, the body and the plastic layers as setupMaterials sets them", () => {
  const p = pasteMaterial3d({ r: 0.5, g: 0.5, b: 0.5 });
  near(p.specular.r, 0.25);
  near(p.shininess, 0.1 * 128);
  const s = silkMaterial3d({ r: 0.9, g: 0.9, b: 0.9 });
  near(s.specular.r, 0.81 + 0.1);
  near(s.shininess, 0.078125 * 128);
  // The epoxy's own ambient and specular; its diffuse and transparency from the body colour (renderBoardBody).
  const b = boardMaterial3d({ r: 0.2, g: 0.17, b: 0.09, a: 0.9 });
  near(b.ambient.r, 117 / 255);
  near(b.specular.b, 20 / 255);
  near(b.transparency, 0.1);
  const u = plasticMaterial3d({ r: 1, g: 0.5, b: 0 });
  near(u.ambient.r, 0.05);
  near(u.specular.g, 0.35);
});

test("inner copper is drawn only when the body does not hide it (Redraw's opacity_min of 0.8)", () => {
  assert.equal(innerLayersDrawn(true, { a: 0.9 }), false, "KiCad's default body, 90 % opaque");
  assert.equal(innerLayersDrawn(true, { a: 0.8 }), true);
  assert.equal(innerLayersDrawn(false, { a: 1 }), true, "the Board Body row off");
});
