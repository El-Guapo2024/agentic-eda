# Ladder: escalating targets for the schematic/placement/routing loops

Four intents of increasing complexity, each a real, coherent circuit (not a
synthetic stress-test), ending at a student-robotics "control hub". Every
rung `lint`s clean -- verified two ways: `eda lint` itself, and an
independent script checking every non-`nc` pin appears on some net and
every net has >=2 pins (see "Lint-level check" below). None crash. All
four currently stall at the schematic *gate* stage on `pipeline` -- that
stall is entirely layout-level (wire routing/label placement in the
auto-generated schematic drawing), not a defect in the intent files
themselves. That stall, and the specific failing checks, *are* the target
list for the schematic-layout loop agent. Once a rung's schematic gates go
clean, `pipeline` will proceed into placement/routing and that rung
becomes a target for the next loop stage.

Note: the engine's schematic-gate set changed under this exercise (some
in-progress work elsewhere in the repo, outside `examples/ladder/`) between
an earlier pass and this one -- `schematic_column_overflow` is gone and
`schematic_flow_direction`/`schematic_missing_junction` are new. The
numbers below are from the current binary (`./target/release/eda`,
rebuilt fresh) and are reproducible (checked 2-3x per rung).

Rung summaries:

1. **`l1_usb_mcu.yaml`** — USB-C in, 3.3V LDO, small MCU (TSSOP-20) with
   SWD, reset, boot0, status LED. 60x40mm.
2. **`l2_sensor_hub.yaml`** — rung 1 + 6-axis IMU, 4x I2C sensor headers
   on a shared pulled-up bus, 2x RC-filtered analog inputs, buzzer driven
   through an NPN switch. 80x55mm.
3. **`l3_motor_hub.yaml`** — rung 2 + battery input (fuse + P-FET reverse-
   polarity protection), 5V buck converter, a 16-channel I2C PWM driver
   (frees GPIO for many independent PWM lines), 2x single-motor H-bridge
   drivers, 4x servo headers on the 5V rail. 100x70mm.
4. **`l4_control_hub.yaml`** — rung 3 + UART I/O co-processor MCU with its
   own USB-UART debug bridge, ESD arrays on both USB ports, a 3rd motor
   driver with per-motor current sense feeding the co-processor's ADCs,
   2 more servo headers (6 total), 8 status LEDs on an I2C GPIO expander,
   a 2nd I2C bus, and an RS-485 field bus transceiver. 120x90mm.

## Results (seed 0, `./target/release/eda`)

### Lint-level check (intent correctness: connectivity, net shape)

| Rung | Parts | Nets | `eda lint` | Nets with <2 pins | Non-`nc` pins on no net |
|---|---|---|---|---|---|
| l1_usb_mcu | 17 | 13 | 0 fail / 0 warn | 0 | 0 |
| l2_sensor_hub | 35 | 24 | 0 fail / 0 warn | 0 | 0 |
| l3_motor_hub | 58 | 44 | 0 fail / 0 warn | 0 | 0 |
| l4_control_hub | 100 | 76 | 0 fail / 0 warn | 0 | 0 |

Every rung was clean at lint level already; no `source_pin_not_connected`,
`source_pin_multiple_nets`, or similar lint fails exist in any of the four
YAMLs. This was double-checked with a script independent of `eda lint`
that walks every part's pins and every net's pin list directly against the
YAML (results above, right two columns) -- confirming every non-`nc` pin
sits on exactly one net and every net has at least 2 pins.

### Layout-level check (schematic-gate outcome, on top of a clean lint)

| Rung | `schematic` gate fails / warns | Failing checks (all layout, none lint) | `pipeline` outcome | Wall time (schematic) |
|---|---|---|---|---|
| l1_usb_mcu | 10 fail / 1 warn | `schematic_flow_direction` (2), `schematic_label_over_wire` (8) | stalls at schematic (exit 1); placement/routing not reached | 0.03s |
| l2_sensor_hub | 10 fail / 1 warn | `schematic_flow_direction` (5), `schematic_label_over_wire` (5) | stalls at schematic (exit 1); placement/routing not reached | 0.09s |
| l3_motor_hub | 28 fail / 0 warn | `schematic_flow_direction` (5), `schematic_label_over_wire` (15), `schematic_text_overlap` (2), `schematic_wire_crossing_count` (1), `schematic_wire_overlap` (4), `schematic_wire_through_symbol` (1) | stalls at schematic (exit 1); placement/routing not reached | 1.07s |
| l4_control_hub | 113 fail / 0 warn | `schematic_flow_direction` (7), `schematic_label_over_wire` (73), `schematic_missing_junction` (1), `schematic_text_overlap` (2), `schematic_wire_crossing_count` (1), `schematic_wire_detour` (3), `schematic_wire_overlap` (20), `schematic_wire_through_symbol` (6) | stalls at schematic (exit 1); placement/routing not reached | 5.04s |

Nothing crashed at any rung/seed 0 for `lint`, `schematic`, or `pipeline`.
`pipeline` hard-stops after the schematic stage whenever `check_schematic`
returns any `Fail` (see `crates/cli/src/main.rs::stage_schematic`), so none
of the four rungs currently reach `place`/`route` via `pipeline` -- every
fail above is a schematic-*drawing* defect (wire crossing a label, a
connector placed against the intended left-to-right signal flow, a wire
run through another symbol's box, etc.), not a problem with the intent's
connectivity. The dominant recurring failure across the ladder is
`schematic_label_over_wire` (a rendered text label overlapping a wire
segment), which scales sharply with part/net count -- 8 -> 5 -> 15 -> 73
across the four rungs -- making it the clearest target for a
schematic-layout loop agent to fix first.

To reproduce:

```
./target/release/eda lint examples/ladder/<rung>.yaml
./target/release/eda schematic examples/ladder/<rung>.yaml -o /tmp/out --seed 0
./target/release/eda pipeline examples/ladder/<rung>.yaml -o /tmp/out --seed 0
```

## Footprint library

No changes were made to `crates/model/src/footprint.rs` or the built-in
package library -- every part resolves through existing builtins
(`0402`/`0603`/`0805`/`1206`/`1210`, `SOT-23`, `SOT-23-6`, `SOT-223`,
`SOIC-8`, `SOIC-16`, `TSSOP-20`, and the generic `PINHEADER-N` family).
Several real ICs with no matching library footprint are represented by a
documented stand-in (see the header comment in each rung's YAML for the
specific rationale per part):

- USB-C receptacles (J1, and the debug port in rung 4) -> `PINHEADER-16`
  / `PINHEADER-4` (pad-count stand-ins; no USB-C footprint in the
  library).
- 6-axis IMU (LGA-14, e.g. ICM-42688-P) -> `SOIC-16`, truncated to the 14
  real signals.
- Reset/boot momentary switches -> `1206` (no SPST-button footprint).
- Magnetic buzzer -> `1210` (no buzzer footprint).
- 16-channel I2C PWM driver (e.g. PCA9685, real TSSOP-28) -> `TSSOP-20`,
  truncated to the pins actually used (grows from 13 to 17 used pins as
  the ladder adds more PWM channels).
- Polyfuse and buck inductor -> `1206` / `1210` (no dedicated footprints).
- H-bridge drivers (DRV8871), buck converter (TPS563201), USB-UART bridge
  (CH340C), I2C GPIO expander (PCF8574), RS-485 transceiver (MAX485) all
  use `SOIC-8` directly -- these are real 8-pin parts, no truncation
  needed.
- ESD protection arrays (USBLC6-2SC6) use `SOT-23-6` directly.

The schema (`ConstraintModel`) has no mounting-hole primitive (only
parts/nets/clusters/placement_rules/board), so the brief's optional
"mounting-hole markers if the schema allows" was intentionally omitted
and noted in rung 4's header comment rather than faked with a fictitious
part.

## Files changed

- `examples/ladder/l1_usb_mcu.yaml` (new)
- `examples/ladder/l2_sensor_hub.yaml` (new)
- `examples/ladder/l3_motor_hub.yaml` (new)
- `examples/ladder/l4_control_hub.yaml` (new)
- `examples/ladder/README.md` (new, this file)

No other files were touched; `crates/model/src/footprint.rs` and
`crates/model/tests/kicad_footprints.rs` are unmodified.
