// Registers ts-resolve.mjs (extensionless relative imports -> .ts) for
// `node --experimental-transform-types`; see tools/run-unit-tests-node.sh.
import { register } from "node:module";
register("./ts-resolve.mjs", import.meta.url);
