#!/usr/bin/env python3
"""Coordinator side of the fleet. A queue is a directory tree:

  ROOT/queue/<job>.json     waiting        (job spec + embedded intent yaml)
  ROOT/claimed/<job>.json   taken by a worker (renamed, atomic on one fs)
  ROOT/done/<job>/          result dir rsynced back by the worker

usage: coord.py ROOT add jobs.yaml        expand a sweep spec into queue/
       coord.py ROOT status               counts + per-worker claims
       coord.py ROOT requeue              put stale claims (no done dir) back
"""
import argparse, glob, itertools, json, os, shutil, sys, time
import yaml

def set_path(d, dotted, value):
    keys = dotted.split(".")
    for k in keys[:-1]:
        d = d.setdefault(k, {})
    d[keys[-1]] = value

def main():
    ap = argparse.ArgumentParser(); ap.add_argument("root"); ap.add_argument("cmd", choices=["add", "status", "requeue"])
    ap.add_argument("spec", nargs="?")
    a = ap.parse_args()
    for sub in ("queue", "claimed", "done"):
        os.makedirs(f"{a.root}/{sub}", exist_ok=True)
    if a.cmd == "add":
        spec = yaml.safe_load(open(a.spec))
        knobs = spec.get("knobs") or [{}]
        n = 0
        for (ki, kn), board, seed in itertools.product(enumerate(knobs), spec["boards"], spec.get("seeds", [0])):
            name = f"{spec.get('name','batch')}-{os.path.splitext(os.path.basename(board))[0]}-k{ki}-s{seed}"
            if os.path.exists(f"{a.root}/done/{name}") or os.path.exists(f"{a.root}/queue/{name}.json"):
                continue
            intent = yaml.safe_load(open(board))
            for k, v in kn.items():
                set_path(intent, k, v)
            job = dict(name=name, board=board, seed=seed, knobs=kn, knob_index=ki, batch=spec.get("name", ""),
                       placer=spec.get("placer", "cypress"), intent_yaml=yaml.safe_dump(intent, sort_keys=False),
                       queued_at=time.time())
            tmp = f"{a.root}/queue/.{name}.tmp"
            json.dump(job, open(tmp, "w"))
            os.rename(tmp, f"{a.root}/queue/{name}.json")
            n += 1
        print(f"queued {n}")
    elif a.cmd == "status":
        q = glob.glob(f"{a.root}/queue/*.json"); c = glob.glob(f"{a.root}/claimed/*.json"); d = glob.glob(f"{a.root}/done/*/")
        print(f"queue {len(q)}  claimed {len(c)}  done {len(d)}")
        for f in sorted(c):
            j = json.load(open(f))
            print(f"  {j.get('worker','?'):14} {os.path.basename(f)[:-5]}  {int(time.time()-j.get('claimed_at',time.time()))} s")
    elif a.cmd == "requeue":
        n = 0
        for f in glob.glob(f"{a.root}/claimed/*.json"):
            name = os.path.basename(f)[:-5]
            if not os.path.exists(f"{a.root}/done/{name}/result.json"):
                j = json.load(open(f)); j.pop("worker", None); j.pop("claimed_at", None)
                json.dump(j, open(f, "w")); os.rename(f, f"{a.root}/queue/{name}.json"); n += 1
        print(f"requeued {n}")

if __name__ == "__main__":
    main()
