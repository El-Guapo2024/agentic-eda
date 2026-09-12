# Fleet: many machines, one queue, no LLM in the loop

Workers are `eda solve` processes. The coordinator is a directory. Ranking is
a script. An LLM (or a human) reads `rank.py` output and writes the next job
file; that is the only place judgement enters.

```
coordinator (any machine with ssh in, e.g. this laptop)
  ROOT/queue/*.json  ->  ROOT/claimed/*.json  ->  ROOT/done/<job>/{result.json,log.txt,judge_*.png,...}

workers (Mac minis, cloud boxes)                         claim = atomic rename over ssh
  worker.py user@coord:ROOT --docker agentic-eda -j 8    run eda solve + judge, rsync dir back
```

## Commands

| step | command |
|---|---|
| expand a sweep spec into jobs | `coord.py ROOT add tools/sweep/ladder.yaml` |
| watch | `coord.py ROOT status` |
| a worker died mid-job | `coord.py ROOT requeue` |
| run jobs on this machine | `worker.py ROOT --eda target/release/eda -j 6` |
| run jobs on a mini | `worker.py user@laptop:ROOT --docker agentic-eda` |
| rank knob sets, best per board | `rank.py ROOT/done [--csv runs.csv]` |
| single-machine shortcut, no queue | `sweep.py tools/sweep/ladder.yaml -o OUT -j 6` |

Job spec (`ladder.yaml`): `boards`, `seeds`, `knobs` (list of dotted overrides
into the intent: `solver.*`, `board.tuning.*`), `name`, `placer`.

## New worker

`bash tools/sweep/setup_worker.sh user@coord:ROOT` installs colima+docker on a
Mac (a Linux VM at native arch, no Rosetta), builds the image for that arch,
checks ssh to the coordinator. Then run `worker.py` under nohup or launchd.
Minis need: ssh key to the coordinator, Tailscale or LAN. Nothing else.

## Numerics

Cypress results differ between x86 and arm64 and between native and Docker.
Same engine, same gates, different arithmetic: a seed is not a design id
across machines. Treat every done dir as its own sample; never assume a seed
reproduces on another worker. `job.json` and `log.txt` record the worker.

## What this is in RL terms

reset = intent yaml, step = knob set + seed, reward = gates pass + score.
Today the policy is "sweep and keep the winners" (rank.py). Thousands of done
dirs are the training set for anything learned later.
