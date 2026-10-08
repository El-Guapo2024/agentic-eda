// Minimal ambient declarations for the subset of Node's built-in
// `node:test` / `node:assert/strict` this repo's unit tests actually use.
// The project takes no new npm packages (so no `@types/node`), and Node
// v20 can't strip TypeScript itself -- this is only so `tsc -p
// tsconfig.test.json` can type-check test files; the real implementation
// is Node's own built-in module, resolved at `node --test` run time the
// normal way. Extend this file if a test starts using another
// node:test/node:assert export; don't reach for @types/node to cover gaps.

declare module "node:test" {
  type TestFn = () => void | Promise<void>;
  export function test(name: string, fn: TestFn): void;
}

declare module "node:assert/strict" {
  interface Assert {
    equal(actual: unknown, expected: unknown, message?: string): void;
    notEqual(actual: unknown, expected: unknown, message?: string): void;
    deepEqual(actual: unknown, expected: unknown, message?: string): void;
    notDeepEqual(actual: unknown, expected: unknown, message?: string): void;
    ok(value: unknown, message?: string): asserts value;
    match(actual: string, expected: RegExp, message?: string): void;
  }
  const assert: Assert;
  export default assert;
}

// Reading a fixture file shared with the Rust tests (src/kicad/*.json): the unit-test build has no JSON module import.
declare module "node:fs" {
  export function readFileSync(path: string, encoding: "utf8"): string;
}
