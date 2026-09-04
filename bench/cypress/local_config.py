"""Emit a DREAMPlace/Cypress config for one Bookshelf .aux, CPU mode.
usage: local_config.py <aux path> <result_dir> <seed>"""
import json, sys
aux, result_dir, seed = sys.argv[1], sys.argv[2], int(sys.argv[3])
bins = 64
print(json.dumps({
    "aux_input": aux,
    "gpu": 0,
    "num_bins_x": bins, "num_bins_y": bins,
    "global_place_stages": [{
        "num_bins_x": bins, "num_bins_y": bins, "iteration": 1000,
        "learning_rate": 0.00025, "wirelength": "weighted_average", "optimizer": "nesterov",
        "Llambda_density_weight_iteration": 1, "Lsub_iteration": 1}],
    "target_density": 0.6, "density_weight": 0.008, "gamma": 0.1318231577,
    "random_seed": seed, "scale_factor": 1.0, "ignore_net_degree": 100,
    "enable_fillers": 1, "gp_noise_ratio": 0.025, "global_place_flag": 1,
    "legalize_flag": 1, "detailed_place_flag": 0, "stop_overflow": 0.07,
    "dtype": "float32", "plot_flag": 0, "result_dir": result_dir,
}, indent=2))
