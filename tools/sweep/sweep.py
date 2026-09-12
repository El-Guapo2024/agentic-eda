#!/usr/bin/env python3
"""Fan out `eda solve` over boards × seeds × knob sets. One dir per job.

usage: sweep.py jobs.yaml -o OUT [-j N] [--eda PATH] [--docker IMAGE]

jobs.yaml:
  boards: [examples/ladder/l1_usb_mcu.yaml, ...]
  seeds: [0, 1, 2]
  placer: cypress
  knobs:                       # list of override sets; {} = defaults
    - {}
    - solver.fit_board_utilization: 0.30
      board.tuning.via_cost_cells: 20
  name: edge4                  # batch name stamped into every job.json

Each job dir holds: intent.yaml (patched copy), job.json (knobs, seed,
board, batch), log.txt (exit=, wall_s=), result.json from eda, judge pngs.
Workers are plain processes; the coordinator is rank.py. No LLM in the loop.
"""
import argparse, copy, re, itertools, json, os, shutil, subprocess, sys, time
from concurrent.futures import ThreadPoolExecutor, as_completed
import yaml

def set_path(d, dotted, value):
    keys = dotted.split(".")
    for k in keys[:-1]:
        d = d.setdefault(k, {})
    d[keys[-1]] = value

def job_name(board, seed, ki):
    return f"{os.path.splitext(os.path.basename(board))[0]}-k{ki}-s{seed}"

def run_job(job, args):
    d = job["dir"]; os.makedirs(d, exist_ok=True)
    intent = yaml.safe_load(open(job["board"]))
    for k, v in job["knobs"].items():
        set_path(intent, k, v)
    yaml.safe_dump(intent, open(f"{d}/intent.yaml", "w"), sort_keys=False)
    json.dump({k: job[k] for k in ("board", "seed", "knobs", "knob_index", "batch")}, open(f"{d}/job.json", "w"), indent=1)
    if args.docker:
        base = ["docker", "run", "--rm", "--platform", "linux/amd64", "-v", f"{os.path.abspath(d)}:/w",
                "-v", f"{os.path.abspath(os.path.dirname(job['board']))}:/examples:ro", args.docker]
        solve = base + ["solve", "/w/intent.yaml", "-o", "/w", "--seed", str(job["seed"]), "--placer", job["placer"]]
        judge = base + ["judge", "/w/intent.yaml", "--design", "/w/design.json", "-o", "/w"]
    else:
        solve = [args.eda, "solve", f"{d}/intent.yaml", "-o", d, "--seed", str(job["seed"]), "--placer", job["placer"]]
        judge = [args.eda, "judge", f"{d}/intent.yaml", "--design", f"{d}/design.json", "-o", d]
    t0 = time.time()
    with open(f"{d}/log.txt", "w") as log:
        rc = subprocess.call(solve, stdout=log, stderr=subprocess.STDOUT)
        log.write(f"\nexit={rc}\nwall_s={int(time.time() - t0)}\n")
    if os.path.exists(f"{d}/design.json"):
        with open(f"{d}/judge.txt", "w") as jl:
            subprocess.call(judge, stdout=jl, stderr=subprocess.STDOUT)
    verdict = "pass" if rc == 0 else next((l.strip() for l in open(f"{d}/log.txt") if re.match(r"^(FAIL|fail)\b", l)), f"exit {rc}")
    return f"{os.path.basename(d)}: {verdict[:100]} ({int(time.time() - t0)} s)"

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("jobs"); ap.add_argument("-o", "--out", required=True)
    ap.add_argument("-j", type=int, default=4)
    ap.add_argument("--eda", default=os.environ.get("EDA_BIN", "target/release/eda"))
    ap.add_argument("--docker", default=None, help="run inside this image instead of --eda")
    ap.add_argument("--fresh", action="store_true", help="wipe OUT first")
    args = ap.parse_args()
    spec = yaml.safe_load(open(args.jobs))
    knobs = spec.get("knobs") or [{}]
    if args.fresh and os.path.isdir(args.out):
        shutil.rmtree(args.out)
    os.makedirs(args.out, exist_ok=True)
    jobs = []
    for (ki, kn), board, seed in itertools.product(enumerate(knobs), spec["boards"], spec.get("seeds", [0])):
        jobs.append(dict(board=board, seed=seed, knobs=kn, knob_index=ki, batch=spec.get("name", ""),
                         placer=spec.get("placer", "cypress"), dir=f"{args.out}/{job_name(board, seed, ki)}"))
    jobs = [j for j in jobs if not os.path.exists(f"{j['dir']}/result.json")]   # resumable
    print(f"{len(jobs)} jobs, {args.j} parallel, out={args.out}", flush=True)
    with ThreadPoolExecutor(args.j) as ex:
        for f in as_completed([ex.submit(run_job, j, args) for j in jobs]):
            print(f.result(), flush=True)
    print("SWEEP_DONE")

if __name__ == "__main__":
    main()
