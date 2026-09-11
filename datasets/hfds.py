#!/usr/bin/env python3
"""
hfds - lazy Hugging Face dataset file browser/fetcher.

Uses huggingface_hub.HfFileSystem (fsspec) so that listing and reading files
inside a dataset repo does NOT require downloading the whole repo / any tars.
A local on-disk cache lives under datasets/cache/ (huggingface_hub's own
blob cache, redirected here via HF_HOME).

This is the "FUSE-style service" in userspace form: every call is a lazy,
on-demand fetch of just the bytes needed. See mount.sh for the real FUSE
mount (Linux-in-Docker primary path; macOS needs macFUSE, optional).

Commands:
    hfds ls <repo> [path]
    hfds find <repo> --ext .kicad_pcb [--path-prefix sub/dir]
    hfds cat <repo> <path>
    hfds sample <repo> --ext .kicad_pcb -n 20 --out DIR [--max-bytes N]

<repo> is given as "datasets/<org>/<name>" (HfFileSystem convention) or
just "<org>/<name>" (the "datasets/" prefix is added automatically).

Hard-fails (no silent fallback) when:
    - the repo is gated/private and no HF_TOKEN grants access
    - the repo or path does not exist
    - `find` matches zero files for the given extension
"""
import argparse
import os
import random
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
CACHE_DIR = SCRIPT_DIR / "cache"
CACHE_DIR.mkdir(parents=True, exist_ok=True)
os.environ.setdefault("HF_HOME", str(CACHE_DIR))

try:
    from huggingface_hub import HfFileSystem, hf_hub_download
    from huggingface_hub.utils import GatedRepoError, RepositoryNotFoundError
except ImportError as e:
    sys.stderr.write(
        "ERROR: huggingface_hub not installed in this interpreter.\n"
        "Run: pip install huggingface_hub fsspec  (or use datasets/.venv)\n"
    )
    raise SystemExit(2) from e


def norm_repo(repo: str) -> tuple[str, str]:
    """Return (fs_path_prefix, repo_id) e.g. ('datasets/org/name', 'org/name')."""
    if repo.startswith("datasets/"):
        repo_id = repo[len("datasets/"):]
    else:
        repo_id = repo
    return f"datasets/{repo_id}", repo_id


def get_fs() -> "HfFileSystem":
    return HfFileSystem()


def fail(msg: str, code: int = 1):
    sys.stderr.write(f"ERROR: {msg}\n")
    raise SystemExit(code)


def cmd_ls(args):
    fs_prefix, repo_id = norm_repo(args.repo)
    path = f"{fs_prefix}/{args.path}".rstrip("/") if args.path else fs_prefix
    fs = get_fs()
    try:
        entries = fs.ls(path, detail=True)
    except (GatedRepoError,) as e:
        fail(f"dataset '{repo_id}' is GATED - cannot list without access approval: {e}")
    except (RepositoryNotFoundError, FileNotFoundError) as e:
        fail(f"repo or path not found: {repo_id}:{args.path!r} ({e})")
    if not entries:
        fail(f"no entries under {repo_id}:{args.path!r} (empty)")
    for e in entries:
        name = e["name"].split("/", 2)[-1] if "/" in e["name"] else e["name"]
        kind = "DIR " if e.get("type") == "directory" else "FILE"
        size = e.get("size", 0)
        print(f"{kind}  {size:>12}  {e['name']}")


def _walk_find(fs, root, ext, path_prefix=None):
    stack = [root]
    matches = []
    while stack:
        cur = stack.pop()
        try:
            entries = fs.ls(cur, detail=True)
        except Exception:
            continue
        for e in entries:
            name = e["name"]
            if e.get("type") == "directory":
                stack.append(name)
            else:
                if path_prefix and path_prefix not in name:
                    continue
                if name.lower().endswith(ext.lower()):
                    matches.append(e)
    return matches


def cmd_find(args):
    fs_prefix, repo_id = norm_repo(args.repo)
    fs = get_fs()
    try:
        matches = _walk_find(fs, fs_prefix, args.ext, args.path_prefix)
    except GatedRepoError as e:
        fail(f"dataset '{repo_id}' is GATED: {e}")
    if not matches:
        fail(f"no files matching '{args.ext}' found in {repo_id} "
             f"(hard fail - not silently returning empty)")
    for m in matches:
        print(f"{m.get('size', 0):>12}  {m['name']}")
    print(f"# {len(matches)} file(s) matching {args.ext}", file=sys.stderr)


def cmd_cat(args):
    fs_prefix, repo_id = norm_repo(args.repo)
    path = f"{fs_prefix}/{args.path}"
    fs = get_fs()
    try:
        with fs.open(path, "rb") as f:
            data = f.read()
    except GatedRepoError as e:
        fail(f"dataset '{repo_id}' is GATED: {e}")
    except FileNotFoundError as e:
        fail(f"file not found: {path} ({e})")
    sys.stdout.buffer.write(data)


def cmd_sample(args):
    fs_prefix, repo_id = norm_repo(args.repo)
    fs = get_fs()
    try:
        matches = _walk_find(fs, fs_prefix, args.ext, args.path_prefix)
    except GatedRepoError as e:
        fail(f"dataset '{repo_id}' is GATED: {e}")
    if not matches:
        fail(f"no files matching '{args.ext}' found in {repo_id} - cannot sample")
    if args.max_bytes:
        matches = [m for m in matches if m.get("size", 0) <= args.max_bytes]
        if not matches:
            fail(f"all matches exceed --max-bytes {args.max_bytes}")
    random.seed(args.seed)
    n = min(args.n, len(matches))
    picked = random.sample(matches, n)
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    downloaded = []
    for m in picked:
        rel = m["name"].split(f"{fs_prefix}/", 1)[-1]
        local_name = rel.replace("/", "__")
        dest = out_dir / local_name
        hf_hub_download(
            repo_id=repo_id,
            repo_type="dataset",
            filename=rel,
            local_dir=str(out_dir / "_hf_layout"),
        )
        # copy into flat layout for convenience
        src = out_dir / "_hf_layout" / rel
        dest.write_bytes(src.read_bytes())
        downloaded.append(str(dest))
        print(f"fetched {rel} -> {dest} ({m.get('size', 0)} bytes)")
    print(f"# sampled {len(downloaded)}/{n} requested from {repo_id}", file=sys.stderr)


def main():
    ap = argparse.ArgumentParser(prog="hfds", description=__doc__,
                                  formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_ls = sub.add_parser("ls", help="list a path in a dataset repo")
    p_ls.add_argument("repo")
    p_ls.add_argument("path", nargs="?", default="")
    p_ls.set_defaults(func=cmd_ls)

    p_find = sub.add_parser("find", help="recursively find files by extension")
    p_find.add_argument("repo")
    p_find.add_argument("--ext", required=True)
    p_find.add_argument("--path-prefix", default=None)
    p_find.set_defaults(func=cmd_find)

    p_cat = sub.add_parser("cat", help="print file contents to stdout")
    p_cat.add_argument("repo")
    p_cat.add_argument("path")
    p_cat.set_defaults(func=cmd_cat)

    p_sample = sub.add_parser("sample", help="download a random sample of matching files")
    p_sample.add_argument("repo")
    p_sample.add_argument("--ext", required=True)
    p_sample.add_argument("-n", type=int, default=10)
    p_sample.add_argument("--out", required=True)
    p_sample.add_argument("--path-prefix", default=None)
    p_sample.add_argument("--max-bytes", type=int, default=None)
    p_sample.add_argument("--seed", type=int, default=0)
    p_sample.set_defaults(func=cmd_sample)

    args = ap.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
