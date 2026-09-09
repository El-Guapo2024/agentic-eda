"""Emit a DREAMPlace/Cypress config for one Bookshelf .aux, CPU mode.
usage: local_config.py <aux path> <result_dir> <seed>"""
import json, sys
aux, result_dir, seed = sys.argv[1], sys.argv[2], int(sys.argv[3])
bins = 64
import os
stop = float(os.environ.get("CYPRESS_STOP_OVERFLOW", "0.30"))
print(json.dumps({
    "aux_input": aux,
    "gpu": 0,
    "num_bins_x": bins, "num_bins_y": bins,
    # mirrors eda-cypress::config_json (Cypress PCB tuner cadence)
    "global_place_stages": [{
        "num_bins_x": bins, "num_bins_y": bins, "iteration": 3000,
        "learning_rate": 0.0038, "learning_rate_decay": 0.993,
        "wirelength": "weighted_average", "optimizer": "nesterov",
        "Llambda_density_weight_iteration": 10, "Lsub_iteration": 2}],
    "target_density": 0.5, "density_weight": 0.0237, "gamma": 0.44,
    "macro_overlap_flag": 1, "macro_overlap_weight": 8e-6, "macro_halo_x": 1, "macro_halo_y": 1,
    "enable_rotation": 1,
    "random_seed": seed, "scale_factor": 1.0, "ignore_net_degree": 100,
    "enable_fillers": 1, "gp_noise_ratio": 0.025, "global_place_flag": 1,
    "legalize_flag": 1, "detailed_place_flag": 0, "stop_overflow": stop,
    "dtype": "float32", "plot_flag": 0, "result_dir": result_dir,
    "deterministic_flag": 1, "num_threads": 1,
}, indent=2))
