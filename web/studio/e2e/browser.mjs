import { chromium } from process.env.PLAYWRIGHT_MODULE || "/opt/node22/lib/node_modules/playwright/index.mjs";
import { writeFileSync } from "node:fs";
export async function open() {
  const b = await chromium.launch({ args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
  const p = await b.newPage({ viewport: { width: 1600, height: 1000 } });
  const errors = [];
  p.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  p.on("pageerror", (e) => errors.push("PAGEERROR " + e.message));
  const cdp = await p.context().newCDPSession(p);
  const shot = async (name) => { const { data } = await cdp.send("Page.captureScreenshot", { format: "png" }); writeFileSync(name, Buffer.from(data, "base64")); };
  return { b, p, errors, shot };
}
