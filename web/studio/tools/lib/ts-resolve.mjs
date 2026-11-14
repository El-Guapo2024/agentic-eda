// Node module-resolution hook: maps an extensionless relative import
// ("./junctions") to its .ts/.tsx file, as tsc/vite resolve it.
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
export async function resolve(spec, ctx, next) {
  // a `.json` import written as vite/tsc take it (no `with { type: "json" }`): node needs the attribute
  if (spec.endsWith(".json")) {
    const r = await next(spec, { ...ctx, importAttributes: { ...ctx.importAttributes, type: "json" } });
    return { ...r, importAttributes: { ...r.importAttributes, type: "json" } };
  }
  if ((spec.startsWith("./") || spec.startsWith("../")) && !/\.[cm]?[jt]sx?$/.test(spec)) {
    const base = new URL(spec, ctx.parentURL);
    for (const ext of [".ts", ".tsx", "/index.ts"]) {
      const u = new URL(base.href + ext);
      if (existsSync(fileURLToPath(u))) return next(u.href, ctx);
    }
  }
  return next(spec, ctx);
}
