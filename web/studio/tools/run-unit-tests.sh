#!/usr/bin/env bash
# Compiles the dependency-free "pure port" TS modules (tsconfig.test.json)
# to a scratch CommonJS directory and runs them with Node's built-in test
# runner, alongside the plain-JS tools/lib/*.test.js files (no compile
# step needed for those). No new npm packages: Node v20 can't strip
# TypeScript itself, so this is the repo's stand-in for `vitest`/`jest`.
set -euo pipefail
cd "$(dirname "$0")/.."

OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

node_modules/.bin/tsc -p tsconfig.test.json --outDir "$OUT"
node --test "$OUT" tools/lib/*.test.js
