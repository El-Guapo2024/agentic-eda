#!/usr/bin/env python3
"""Coordinator step 1: table every job in a sweep dir, rank knob sets.

usage: rank.py OUT [OUT2 ...] [--csv file]
Ranks knob sets by pass rate, then mean vias, then mean track length,
over the boards they were tried on. Prints per-board best run too.
"""
import argparse, glob, json, os, re, statistics as st, csv, sys

def load(d):
    job = json.load(open(f"{d}/job.json")) if os.path.exists(f"{d}/job.json") else {}
    res = json.load(open(f"{d}/result.json")) if os.path.exists(f"{d}/result.json") else {}
    log = open(f"{d}/log.txt").read() if os.path.exists(f"{d}/log.txt") else ""
    ok = bool(re.search(r"^exit=0", log, re.M))
    wall = re.search(r"^wall_s=(\d+)", log, re.M)
    fail = next((l.strip() for l in log.splitlines() if re.match(r"^(FAIL|fail)\b", l)), "")
    s = res.get("score") or {}
    return dict(dir=d, board=os.path.basename(job.get("board", "?")).split(".")[0], seed=job.get("seed"),
                k=job.get("knob_index"), knobs=job.get("knobs", {}), ok=ok, wall=int(wall.group(1)) if wall else None,
                vias=s.get("vias"), track=s.get("track_len_um"), use=s.get("board_use"), detour=s.get("detour_max"),
                fail=fail.split(" @ ")[0][5:].strip())

def main():
    ap = argparse.ArgumentParser(); ap.add_argument("outs", nargs="+"); ap.add_argument("--csv")
    a = ap.parse_args()
    runs = [load(d) for o in a.outs for d in sorted(glob.glob(f"{o}/*")) if os.path.isdir(d)]
    if a.csv:
        w = csv.DictWriter(open(a.csv, "w"), fieldnames=list(runs[0].keys())); w.writeheader(); w.writerows(runs)
    print(f"{'job':34} {'ok':3} {'wall':>5} {'vias':>4} {'track':>7} {'use':>4} {'det':>4}  fail")
    for r in runs:
        print(f"{os.path.basename(r['dir']):34} {'ok' if r['ok'] else '--':3} {r['wall'] or 0:5} {r['vias'] or 0:4} "
              f"{(r['track'] or 0)//1000:6}mm {round(100*(r['use'] or 0)):3}% {r['detour'] or 0:4}  {r['fail']}")
    print("\nknob sets, ranked:")
    by_k = {}
    for r in runs:
        by_k.setdefault(r["k"], []).append(r)
    rows = []
    for k, rs in by_k.items():
        ok = [r for r in rs if r["ok"]]
        rows.append((len(ok) / len(rs), st.mean([r["vias"] for r in ok]) if ok else 1e9,
                     st.mean([r["track"] for r in ok]) if ok else 1e9, k, len(ok), len(rs), rs[0]["knobs"]))
    for pr, v, t, k, n_ok, n, knobs in sorted(rows, key=lambda x: (-x[0], x[1], x[2])):
        print(f"  k{k}: {n_ok}/{n} pass, vias {v if v < 1e9 else '-'}, track {int(t)//1000 if t < 1e9 else '-'} mm  {knobs}")
    print("\nbest per board:")
    for b in sorted({r["board"] for r in runs}):
        rs = [r for r in runs if r["board"] == b and r["ok"]]
        if rs:
            best = min(rs, key=lambda r: (r["vias"], r["track"]))
            print(f"  {b}: {os.path.basename(best['dir'])} vias {best['vias']} track {best['track']//1000} mm")
        else:
            print(f"  {b}: no passing run")

if __name__ == "__main__":
    main()
