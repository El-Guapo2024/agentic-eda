// Node module-resolution hook: maps an extensionless relative import
// ("./junctions") to its .ts/.tsx file, as tsc/vite resolve it.
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
export async function resolve(spec, ctx, next) {
  if ((spec.startsWith("./") || spec.startsWith("../")) && !/\.[cm]?[jt]sx?$/.test(spec)) {
    const base = new URL(spec, ctx.parentURL);
    for (const ext of [".ts", ".tsx", "/index.ts"]) {
      const u = new URL(base.href + ext);
      if (existsSync(fileURLToPath(u))) return next(u.href, ctx);
    }
  }
  return next(spec, ctx);
}
