# KiCad export bench report

Generated 2026-09-03T21:54:37Z by `bench/kicad/run.sh` (cargo build profile: debug).

kicad-cli: /Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli

| case | seed | pipeline | ERC violations | DRC violations (in-scope: clearance/track_width/shorting_items/unconnected_items) | DRC violations (other, informational) | notes |
|---|---|---|---|---|---|---|
| all_power_ground_net | 0 | ok | 0 | 0 | 5 |  |
| all_power_ground_net | 1 | ok | 0 | 0 | 5 |  |
| all_power_ground_net | 2 | ok | 0 | 0 | 5 |  |
| all_power_ground_net | 3 | ok | 0 | 0 | 5 |  |
| dense_small_outline | 0 | ok | 0 | 0 | 14 |  |
| dense_small_outline | 1 | ok | 0 | 0 | 13 |  |
| dense_small_outline | 2 | ok | 0 | 0 | 14 |  |
| dense_small_outline | 3 | ok | 0 | 0 | 15 |  |
| ldo_proximity_heavy | 0 | ok | 0 | 0 | 19 |  |
| ldo_proximity_heavy | 1 | ok | 0 | 0 | 20 |  |
| ldo_proximity_heavy | 2 | ok | 0 | 0 | 21 |  |
| ldo_proximity_heavy | 3 | ok | 0 | 0 | 21 |  |
| ldo | 0 | ok | 0 | 0 | 14 |  |
| ldo | 1 | ok | 0 | 0 | 16 |  |
| ldo | 2 | ok | 0 | 0 | 16 |  |
| ldo | 3 | ok | 0 | 0 | 15 |  |
| mcu_board_30plus | 0 | ok | 0 | 0 | 73 |  |
| mcu_board_30plus | 1 | ok | 0 | 0 | 73 |  |
| mcu_board_30plus | 2 | ok | 0 | 0 | 70 |  |
| mcu_board_30plus | 3 | ok | 0 | 0 | 78 |  |
| nc_pins | 0 | ok | 0 | 0 | 6 |  |
| nc_pins | 1 | ok | 0 | 0 | 5 |  |
| nc_pins | 2 | ok | 0 | 0 | 6 |  |
| nc_pins | 3 | ok | 0 | 0 | 5 |  |
| opamp_filter | 0 | ok | 0 | 0 | 15 |  |
| opamp_filter | 1 | ok | 0 | 0 | 18 |  |
| opamp_filter | 2 | ok | 0 | 0 | 18 |  |
| opamp_filter | 3 | ok | 0 | 0 | 15 |  |
| passive_divider_ladder | 0 | ok | 0 | 0 | 14 |  |
| passive_divider_ladder | 1 | ok | 0 | 0 | 17 |  |
| passive_divider_ladder | 2 | ok | 0 | 0 | 16 |  |
| passive_divider_ladder | 3 | ok | 0 | 0 | 17 |  |
| star_net | 0 | ok | 0 | 0 | 19 |  |
| star_net | 1 | ok | 0 | 0 | 19 |  |
| star_net | 2 | ok | 0 | 0 | 18 |  |
| star_net | 3 | ok | 0 | 0 | 18 |  |
| through_hole_headers | 0 | ok | 0 | 0 | 13 |  |
| through_hole_headers | 1 | ok | 0 | 0 | 14 |  |
| through_hole_headers | 2 | ok | 0 | 0 | 13 |  |
| through_hole_headers | 3 | ok | 0 | 0 | 13 |  |
| two_pin_nets | 0 | ok | 0 | 0 | 7 |  |
| two_pin_nets | 1 | ok | 0 | 0 | 8 |  |
| two_pin_nets | 2 | ok | 0 | 0 | 7 |  |
| two_pin_nets | 3 | ok | 0 | 0 | 8 |  |
| unroutable_tiny_outline | 0 | FAIL | - | - | - | pipeline failed, see `/var/folders/h1/4z3hks0d2zbd_y_lng5ww0680000gn/T//eda_bench_kicad.ZEqYoQ/unroutable_tiny_outline_0/pipeline.log` |
| unroutable_tiny_outline | 1 | FAIL | - | - | - | pipeline failed, see `/var/folders/h1/4z3hks0d2zbd_y_lng5ww0680000gn/T//eda_bench_kicad.ZEqYoQ/unroutable_tiny_outline_1/pipeline.log` |
| unroutable_tiny_outline | 2 | FAIL | - | - | - | pipeline failed, see `/var/folders/h1/4z3hks0d2zbd_y_lng5ww0680000gn/T//eda_bench_kicad.ZEqYoQ/unroutable_tiny_outline_2/pipeline.log` |
| unroutable_tiny_outline | 3 | FAIL | - | - | - | pipeline failed, see `/var/folders/h1/4z3hks0d2zbd_y_lng5ww0680000gn/T//eda_bench_kicad.ZEqYoQ/unroutable_tiny_outline_3/pipeline.log` |
