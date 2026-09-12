#!/usr/bin/env python3
"""Worker: runs on each machine (Mac mini, cloud box, this laptop).
Claims jobs from a coordinator queue, runs `eda solve` + `judge`, ships the
result dir back. Coordinator is reached over ssh (`user@host:/path`) or is a
local path. Claim = rename queue/X.json -> claimed/X.json; rename is atomic
on one filesystem so two workers never take the same job.

usage: worker.py COORD [--eda PATH] [--docker IMAGE] [-j N] [--work DIR] [--once]
  COORD   user@host:/abs/root   or   /abs/root
  -j N    parallel jobs on this machine (default: cores // 2)
"""
import argparse, json, os, shutil, socket, subprocess, sys, time, re
from concurrent.futures import ThreadPoolExecutor

class Coord:
    def __init__(self, spec):
        if ":" in spec and not spec.startswith("/"):
            self.host, self.root = spec.split(":", 1)
        else:
            self.host, self.root = None, spec
    def sh(self, cmd):
        full = cmd if self.host is None else ["ssh", "-o", "BatchMode=yes", self.host, " ".join(cmd)]
        return subprocess.run(full, capture_output=True, text=True, shell=False)
    def claim(self, worker):
        ls = self.sh(["ls", f"{self.root}/queue"])
        names = [n for n in ls.stdout.split() if n.endswith(".json")]
        for n in names:
            if self.sh(["mv", "-n", f"{self.root}/queue/{n}", f"{self.root}/claimed/{n}"]).returncode != 0:
                continue
            # mv -n: if another worker won the race the source is gone and mv fails with status != 0 on linux;
            # on macOS mv -n returns 0 silently, so verify we own it by stamping.
            path = f"{self.root}/claimed/{n}"
            cat = self.sh(["cat", path])
            if cat.returncode != 0 or not cat.stdout.strip():
                continue
            job = json.loads(cat.stdout)
            if job.get("worker"):
                continue
            job["worker"], job["claimed_at"] = worker, time.time()
            stamp = json.dumps(job)
            if self.host is None:
                open(path, "w").write(stamp)
            else:
                subprocess.run(["ssh", self.host, f"cat > {path}"], input=stamp, text=True)
            return job
        return None
    def push(self, local_dir, name):
        dst = f"{self.root}/done/{name}/" if self.host is None else f"{self.host}:{self.root}/done/{name}/"
        if self.host is None:
            os.makedirs(dst, exist_ok=True)
        else:
            self.sh(["mkdir", "-p", f"{self.root}/done/{name}"])
        rc = subprocess.call(["rsync", "-a", "--exclude", "cypress-*", local_dir + "/", dst])
        if rc == 0:
            self.sh(["rm", "-f", f"{self.root}/claimed/{name}.json"])
        return rc

def run(job, args, coord):
    d = f"{args.work}/{job['name']}"
    shutil.rmtree(d, ignore_errors=True); os.makedirs(d)
    open(f"{d}/intent.yaml", "w").write(job["intent_yaml"])
    json.dump({k: job[k] for k in ("name", "board", "seed", "knobs", "knob_index", "batch", "worker")}, open(f"{d}/job.json", "w"), indent=1)
    if args.docker:
        base = ["docker", "run", "--rm", "-v", f"{os.path.abspath(d)}:/w", args.docker]
        solve = base + ["solve", "/w/intent.yaml", "-o", "/w", "--seed", str(job["seed"]), "--placer", job["placer"]]
        judge = base + ["judge", "/w/intent.yaml", "--design", "/w/design.json", "-o", "/w"]
    else:
        solve = [args.eda, "solve", f"{d}/intent.yaml", "-o", d, "--seed", str(job["seed"]), "--placer", job["placer"]]
        judge = [args.eda, "judge", f"{d}/intent.yaml", "--design", f"{d}/design.json", "-o", d]
    t0 = time.time()
    with open(f"{d}/log.txt", "w") as log:
        rc = subprocess.call(solve, stdout=log, stderr=subprocess.STDOUT, cwd=d)
        log.write(f"\nexit={rc}\nwall_s={int(time.time()-t0)}\nworker={job['worker']}\n")
    if os.path.exists(f"{d}/design.json"):
        with open(f"{d}/judge.txt", "w") as jl:
            subprocess.call(judge, stdout=jl, stderr=subprocess.STDOUT, cwd=d)
    if not os.path.exists(f"{d}/result.json"):   # crash: leave a marker so rank.py counts it
        json.dump({"crash": True, "exit": rc}, open(f"{d}/result.json", "w"))
    coord.push(d, job["name"])
    verdict = "pass" if rc == 0 else next((l.strip() for l in open(f"{d}/log.txt") if re.match(r"^(FAIL|fail)\b", l)), f"exit {rc}")
    return f"{job['name']}: {verdict[:90]} ({int(time.time()-t0)} s)"

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("coord"); ap.add_argument("--eda", default=os.environ.get("EDA_BIN", "target/release/eda"))
    ap.add_argument("--docker"); ap.add_argument("-j", type=int, default=max(1, (os.cpu_count() or 2) // 2))
    ap.add_argument("--work", default=os.path.expanduser("~/eda-work")); ap.add_argument("--once", action="store_true")
    ap.add_argument("--idle-s", type=float, default=30)
    args = ap.parse_args()
    args.eda = os.path.abspath(args.eda)
    coord = Coord(args.coord); me = socket.gethostname().split(".")[0]
    os.makedirs(args.work, exist_ok=True)
    print(f"worker {me}, {args.j} slots, coord {args.coord}", flush=True)
    with ThreadPoolExecutor(args.j) as ex:
        futs = set()
        while True:
            futs = {f for f in futs if not f.done() or print(f.result(), flush=True)}
            while len(futs) < args.j:
                job = coord.claim(me)
                if not job:
                    break
                futs.add(ex.submit(run, job, args, coord))
            if not futs:
                if args.once:
                    break
                time.sleep(args.idle_s); continue
            time.sleep(2)
    print("WORKER_IDLE")

if __name__ == "__main__":
    main()
