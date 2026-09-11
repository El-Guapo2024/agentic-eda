#!/usr/bin/env bash
# mount.sh - FUSE-mount a Hugging Face dataset repo as a (lazy, read-only)
# directory tree, via fsspec's FUSE wrapper + fusepy + HfFileSystem.
#
# Files appear instantly on `ls`; bytes are fetched from the Hub only when
# actually read (open/read), and fsspec/huggingface_hub cache blocks locally
# under datasets/cache/ so repeated reads don't re-download.
#
# PRIMARY TARGET: Linux, inside the `agentic-eda` Docker image. FUSE needs a
# kernel driver; the container needs --cap-add SYS_ADMIN --device /dev/fuse
# (see docker-fuse.md in this directory).
#
# ON macOS: the kernel driver is macFUSE (a kernel extension requiring
# System Settings > Privacy & Security approval + reboot). This script will
# refuse to run on macOS unless macFUSE is already installed
# (/Library/Filesystems/macfuse.fs present) - it will NOT prompt for
# installation, since that is a privileged, user-interactive step this repo
# cannot and should not automate.
#
# Usage:
#   ./mount.sh <repo_id> <mountpoint>
#   ./mount.sh bshada/open-schematics /mnt/hf-open-schematics
#
# Unmount:
#   umount <mountpoint>        # Linux
#   umount <mountpoint>        # macOS (same, once macFUSE is active)

set -euo pipefail

REPO="${1:-}"
MOUNTPOINT="${2:-}"

if [[ -z "$REPO" || -z "$MOUNTPOINT" ]]; then
  echo "usage: $0 <repo_id> <mountpoint>" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PYTHON="$SCRIPT_DIR/.venv/bin/python"
if [[ ! -x "$PYTHON" ]]; then
  PYTHON="$(command -v python3)"
fi

OS="$(uname -s)"
if [[ "$OS" == "Darwin" ]]; then
  if [[ ! -e /Library/Filesystems/macfuse.fs ]]; then
    echo "ERROR: macFUSE is not installed on this Mac." >&2
    echo "This is a kernel extension; installing it requires manual user" >&2
    echo "approval in System Settings > Privacy & Security, plus a reboot." >&2
    echo "Install it yourself from https://macfuse.github.io/ if you want to" >&2
    echo "test the mount on macOS, then re-run this script. Otherwise run" >&2
    echo "this inside the Linux Docker image instead (see docker-fuse.md)." >&2
    exit 3
  fi
  echo "macFUSE detected - attempting mount on macOS (best effort)." >&2
fi

mkdir -p "$MOUNTPOINT"

exec "$PYTHON" - "$REPO" "$MOUNTPOINT" <<'PYEOF'
import sys
import os

repo, mountpoint = sys.argv[1], sys.argv[2]

os.environ.setdefault("HF_HOME", os.path.join(os.path.dirname(__file__) or ".", "cache"))

try:
    from huggingface_hub import HfFileSystem
    from fsspec.fuse import run
except ImportError as e:
    sys.stderr.write(
        "ERROR: missing fsspec[fuse]/fusepy or huggingface_hub in this "
        f"interpreter: {e}\n"
        "pip install huggingface_hub fsspec fusepy\n"
    )
    sys.exit(2)

repo_id = repo[len("datasets/"):] if repo.startswith("datasets/") else repo
fs = HfFileSystem()
path = f"datasets/{repo_id}"

print(f"Mounting {path} at {mountpoint} (read-only, lazy fetch)...")
print("Ctrl-C to unmount.")
# fsspec.fuse.run blocks, serving the FUSE loop in foreground.
run(fs, path, mountpoint)
PYEOF
