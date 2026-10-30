# Safety + power board (desktop PCB fab, board 1)

The first board of the user's desktop PCB fab machine. The brief is the user's `cad-demo/ELECTRONICS-BRIEF.md` (on their Mac, not in this repo).

- `gen_safety_power.py` is the source: edit it, then `python3 gen_safety_power.py > safety_power.yaml`.
- `safety_power.yaml` is the generated intent (142 parts, 12 named blocks).
- Build and view the board (it lives in the untracked `work/`):
  ```
  KICAD_PATH=~/src/kicad-libs target/release/eda board new designs/safety-power/safety_power.yaml -o work/safety-power
  KICAD_PATH=~/src/kicad-libs target/release/eda board serve -C work/safety-power --port 8766 --ui web/studio/dist
  ```
  `KICAD_PATH` must point at the KiCad footprint and symbol libraries (see docs/HANDOFF.md, "Parts research tools").

## Status (2026-10-30)

Schematic done: 12 sheets named after the blocks; kicad-cli ERC has 2 warnings, both intended (U13's `~A` and `~CLR` tied straight to the rails). **Not placed, not routed.**

## Spec and decisions

- 2 layers at JLCPCB, about 100 x 80 mm, 24 V only. Mains is off-board (the DIN contactor and mains fuses are in the cabinet; this board drives the contactor coil).
- No microcontroller on this board, on purpose: the safety chain is relays and comparators only. A separate motion MCU board (a bought Klipper board; which one is still open) reads the status and drives "permit" over J16. The Jetson talks to that MCU, not to this board.
- Safety architecture **option A** (the user): a series switch chain (E-stop, door 1, door 2, key, tool lock, fume-fan OK) into the laser source's interlock input, plus a latching relay driving the mains contactor coil with a manual reset. No certified DIN safety module (too expensive).
- The user decided: the E-stop also cuts the motor 24 V (K1) and the toolhead 24 V (K2); the fume-fan tach is required for laser enable, with a jumper bypass (JP1).
- Defaults built in, **still to confirm with the user**: (1) a door opening cuts the laser only, the E-stop cuts everything; (2) the Jetson rail is 19 V (vs 12 V).
- Proposed, **waiting for the user's OK**: the over-temp NTC pull-up from 10k to 1k (signal about 1.7 -> 12 mV/°C at 270 °C, so the LM393's 5 mV offset drops from about 3-5 °C to under 1 °C) and a little hysteresis on the comparator. The user asked whether the LM393 is the best choice: the alternatives in JLC stock (LM2903, TLV1702, TLV1812, TLV3202, LMV393) were compared, and the LM393DR2G (basic, $0.07) stays; the circuit, not the chip, limits the accuracy.

## Parts (each chosen from the JLC database with its datasheet read)

| Function | Part | LCSC |
|---|---|---|
| Input fuse 20 A ATO holder | XF-507P | C668119 |
| Input TVS | SMCJ33CA | C310048 |
| Ideal-diode controller (the user asked for better than LM5050) | MX74700T, SOT-23-6, -100 V, LM74700-compatible | C22468156 |
| Ideal-diode FET | BSC016N06NS | C454269 |
| Bulk caps | 100 uF 35 V | C72478 |
| Motor / toolhead relays | JQC-3FF 24 V | C3638 |
| Bucks 24 -> 19 V and 24 -> 5 V | LMR16030SDDA | C136648 |
| Buck inductor | 10 uH FXL0630 | C167223 |
| Buck diode | SS56 | C24138 |
| Buck caps | 10 uF 50 V 1206 | C13585 |
| Control supply PTC | 1.1 A 1812 | C70119 |
| Laser chain and latch relays (force-guided style) | G5V-2 24 V | C39593 |
| Optocouplers (readback, permit) | EL357N | C42379244 |
| Fan-tach retriggerable one-shot | SN74LVC1G123 | C123302 |
| Over-temp comparator | LM393DR2G | C7955 |
| Input terminal | MKDS 5.08 2P | C8465 |

## Still to check

- The JLC part numbers of the 3.81 mm 4P/2P (Phoenix MC) and the JST XH 12P connectors.
- That the XF-507P fuse holder matches the KiCad ATO footprint.
- Stock of the 4.12k and 17.8k feedback resistors.
- During layout: copper pours for the 20 A path and the high-current outputs.

## Next steps (the user wants to be present and watch in the studio)

1. Placement, stopping for the user's OK after each step: connectors on the edges; then the high-current path (fuse, ideal diode, relays, output fuses); then the bucks; then the safety logic and readback.
2. Routing, with pours on the high-current nets; kicad-cli DRC.
3. ngspice: the over-temp comparator, the SSR gate drive, the fan-tach one-shot, optionally the bucks.
4. BOM review with the user.
