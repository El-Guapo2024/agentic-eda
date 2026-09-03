#!/usr/bin/env bash
# Differential bench: eda-layout (port) vs elkjs (original).
#
# 1. cargo run --example dump_cases -> bench/elk/cases.json (corpus + our
#    layout's results/metrics baked in as `port`).
# 2. node run.mjs -> runs elkjs on the same corpus, adds `elk`
#    results/metrics to each case, writes bench/elk/report.md.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
REPO_ROOT="$(cd ../.. && pwd)"

echo "== generating case corpus with our layout =="
(cd "$REPO_ROOT" && cargo run -q -p eda-layout --example dump_cases -- bench/elk/cases.json)

if [ ! -d node_modules ]; then
  echo "== installing node deps =="
  npm install
fi

echo "== running elkjs and computing report =="
node run.mjs

echo "== done: see bench/elk/report.md =="
