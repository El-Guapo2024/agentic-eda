#!/usr/bin/env bash
# The same "pure port" unit tests as run-unit-tests.sh, without node_modules:
# Node >= 22.7 strips/transforms TypeScript itself, and tools/lib/ts-register.mjs
# resolves this codebase's extensionless relative imports to their .ts files.
# For environments (e.g. a fresh cloud container) where `npm ci` hasn't run.
set -euo pipefail
cd "$(dirname "$0")/.."
node --experimental-transform-types --no-warnings --import ./tools/lib/ts-register.mjs --test \
  src/kicad-port/*.test.ts src/actions/hotkeys.test.ts src/components/schematic/*.test.ts tools/lib/*.test.js
