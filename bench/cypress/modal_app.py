"""Cypress (NVlabs, ISPD'25) on a Modal GPU — the original placer, wrapped.

    pip install modal && modal setup          # once, browser login
    modal run bench/cypress/modal_app.py --bookshelf-dir out/bookshelf --name ldo

The image is built remotely from NVIDIA's PyTorch base with Cypress compiled
for T4 (sm_75) and A10G (sm_86); nothing NVIDIA-shaped runs on the Mac.
`place()` takes the five Bookshelf files, runs `dreamplace/Placer.py`, and
returns the `.gp.pl` text plus the log. Judging (our gates, HPWL, routing)
happens back on the client: see run.py.
"""
import json
import os
import subprocess
import tempfile
from pathlib import Path

import modal

CYPRESS_ARCHS = "-gencode=arch=compute_75,code=sm_75 -gencode=arch=compute_86,code=sm_86"

image = (
    modal.Image.from_registry("nvcr.io/nvidia/pytorch:21.10-py3", add_python=None)
    .apt_install("git", "flex", "bison", "libboost-all-dev", "libcairo2-dev", "cmake", "build-essential")
    .run_commands(
        "git clone --recursive https://github.com/NVlabs/Cypress /Cypress",
        "cd /Cypress && pip install -r requirements.txt",
        "cd /Cypress && mkdir -p build && cd build && "
        f"cmake .. -DCMAKE_INSTALL_PREFIX=/Cypress/install -DPYTHON_EXECUTABLE=$(which python) "
        f"-DCMAKE_CUDA_FLAGS='{CYPRESS_ARCHS}' && make -j8 && make install",
    )
)

app = modal.App("agentic-eda-cypress", image=image)


def cypress_config(name: str, gpu: int, seed: int, target_density: float, bins: int) -> dict:
    """DREAMPlace/Cypress params for a small PCB. Keys from test/simple.json
    and test/tune/pcb-configspace.json (defaults there)."""
    return {
        "aux_input": f"/work/{name}/{name}.aux",
        "gpu": gpu,
        "num_bins_x": bins,
        "num_bins_y": bins,
        "global_place_stages": [
            {
                "num_bins_x": bins,
                "num_bins_y": bins,
                "iteration": 1000,
                "learning_rate": 0.00025,
                "wirelength": "weighted_average",
                "optimizer": "nesterov",
                "Llambda_density_weight_iteration": 1,
                "Lsub_iteration": 1,
            }
        ],
        "target_density": target_density,
        "density_weight": 0.008,
        "gamma": 0.1318231577,
        "random_seed": seed,
        "scale_factor": 1.0,
        "ignore_net_degree": 100,
        "enable_fillers": 1,
        "gp_noise_ratio": 0.025,
        "global_place_flag": 1,
        "legalize_flag": 1,
        "detailed_place_flag": 0,
        "stop_overflow": 0.07,
        "dtype": "float32",
        "plot_flag": 0,
        "result_dir": "/work/results",
    }


@app.function(gpu="T4", timeout=1800)
def place(files: dict, name: str, seed: int = 1000, target_density: float = 0.6, bins: int = 64, gpu: int = 1) -> dict:
    work = Path("/work") / name
    work.mkdir(parents=True, exist_ok=True)
    for fname, content in files.items():
        (work / fname).write_text(content)
    cfg = cypress_config(name, gpu, seed, target_density, bins)
    cfg_path = Path("/work") / f"{name}.json"
    cfg_path.write_text(json.dumps(cfg, indent=2))
    env = dict(os.environ, PYTHONPATH="/Cypress/install")
    proc = subprocess.run(
        ["python", "dreamplace/Placer.py", str(cfg_path)],
        cwd="/Cypress/install",
        env=env,
        capture_output=True,
        text=True,
        timeout=1700,
    )
    out_pl = Path("/work/results") / name / f"{name}.gp.pl"
    return {
        "ok": proc.returncode == 0 and out_pl.exists(),
        "returncode": proc.returncode,
        "pl": out_pl.read_text() if out_pl.exists() else "",
        "log": (proc.stdout + "\n" + proc.stderr)[-20000:],
        "config": cfg,
    }


@app.local_entrypoint()
def main(bookshelf_dir: str, name: str, seed: int = 1000, target_density: float = 0.6, bins: int = 64, gpu: int = 1, out: str = ""):
    d = Path(bookshelf_dir)
    files = {p.name: p.read_text() for p in d.iterdir() if p.suffix in (".aux", ".nodes", ".nets", ".pl", ".scl")}
    res = place.remote(files, name, seed=seed, target_density=target_density, bins=bins, gpu=gpu)
    out_dir = Path(out or d.parent / "cypress")
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / f"{name}.gp.pl").write_text(res["pl"])
    (out_dir / "cypress.log").write_text(res["log"])
    (out_dir / "config.json").write_text(json.dumps(res["config"], indent=2))
    print(f"cypress ok={res['ok']} rc={res['returncode']} -> {out_dir}/{name}.gp.pl")
    if not res["ok"]:
        print(res["log"][-3000:])
        raise SystemExit(1)
