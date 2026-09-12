#!/usr/bin/env bash
# One-shot setup for a fleet worker (Mac mini, Apple Silicon or Intel; Linux box).
# Footprint on the worker: docker, python3, rsync, an ssh key that can reach the
# coordinator. eda + Cypress live inside the image, built natively for this arch.
#
#   bash setup_worker.sh user@coord-host:/abs/path/to/queue-root
#
# Then keep it running:  nohup python3 ~/agentic-eda/tools/sweep/worker.py "$COORD" --docker agentic-eda &
set -euo pipefail
COORD=${1:?coordinator as user@host:/abs/root}
REPO=${REPO:-https://github.com/juanantonioluera/agentic-eda}   # or rsync the tree from the coordinator
if ! command -v docker >/dev/null; then
  if [[ "$(uname)" == Darwin ]]; then
    command -v brew >/dev/null || /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
    brew install colima docker rsync python3
    colima start --cpu "$(sysctl -n hw.ncpu)" --memory 12 --disk 60     # linux VM, native arch, no emulation
  else
    curl -fsSL https://get.docker.com | sh
  fi
fi
[[ -d ~/agentic-eda ]] || git clone "$REPO" ~/agentic-eda
cd ~/agentic-eda
docker build -f docker/Dockerfile -t agentic-eda .                   # ~30 min first time: compiles Cypress for this arch
ssh -o BatchMode=yes "${COORD%%:*}" true || { echo "ssh to coordinator must work without a password (ssh-copy-id)"; exit 1; }
python3 -c "import yaml" 2>/dev/null || pip3 install --user pyyaml
echo "ready: python3 ~/agentic-eda/tools/sweep/worker.py $COORD --docker agentic-eda"
