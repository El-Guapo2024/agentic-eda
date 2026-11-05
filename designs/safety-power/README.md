# Safety + power board (desktop PCB fab, board 1)

The safety and power board for the user's UV-laser desktop PCB fab: a LumenPnP, a JPT Lark-355-5A UV laser, an SG7110 galvo, a JCZ LMCV4 controller, a hot plate, a vacuum bed, and 3× 395 nm UV lamps.

The brief is `projects/pcbfab/ELECTRONICS-BRIEF.md` on branch `pcbfab-demo` of El-Guapo2024/cad-agent, in its 2026-11-01 UV version.

`PARTS.md` lists the machine-level parts: laser, galvo, supplies, lamps, hot plate, contactors and switches. Their datasheets are on the box in `~/datasheets/pcbfab/`.

**Files**
- `gen_safety_power.py` is the source. Edit it, then run `python3 gen_safety_power.py > safety_power.yaml`.
- `safety_power.yaml` is the generated intent: 139 parts in 9 named blocks, plus 4 mounting holes.

**Build and view the board** (it lives in the untracked `work/`):
```
KICAD_PATH=~/src/kicad-libs target/release/eda board new designs/safety-power/safety_power.yaml -o work/safety-power
KICAD_PATH=~/src/kicad-libs target/release/eda board serve -C work/safety-power --port 8766 --ui web/studio/dist
```

## Decisions (the user's)

- **Mains** is 120 V (US). Mains stays off this board. The contactors (Schneider LC1D09BL, 24 V DC coil, mirror NC contact) sit on a DIN rail in the cabinet, and this board drives their coils.
- **Safety chain:** discrete, two channels (option A), using Omron G7SA-3A1B force-guided relays. No certified safety module.
- **Level 1, E-stop:** the E-stop (2 NC), the hot plate's Selco ORA-500-QC cut-out (channel A) and a manual RESET. Its output, RUN, feeds three things:
  - KM1, the hot-plate contactor;
  - K1, motion 24 V;
  - K2, pump and valve 24 V.
- **Level 2, laser and UV lamps:** fed from RUN, through door 1, door 2 and the key switch, then the airflow switch (JP1 bypasses the airflow switch).
  - It drives KMA and KMB, two contactors in series in the mains feed to the laser's 12 V supply and the lamp drivers. The Lark has no interlock input, so its supply's mains is cut instead.
  - The MCU switches the lamps through KML. The HLG drivers dim only to 10 %, and a constant-current driver's output must never be broken.
- **Weld check:** the reset loop passes through every safety relay's NC contact and the contactors' mirror NCs, so a welded contact blocks the reset.
- **MCU interface:** the MCU sees 7 readbacks through optocouplers, and sends 3 commands (permit, lamps, extractor) through opto + FET. The MCU can stop things but can never override the chain.
- **Hot plate:** capped at 240 °C. The heater must be wound for 120 V.
- **Height:** measured with the top camera plus a red line laser, with no distance sensor.
- **Part selection:** economical, but reliable mass-produced parts from established makers with real datasheets.

## Board

- 2 layers, 220 × 160 mm, **2 oz copper**. M3 holes sit in the four corners, and every connector is on an edge.
- **Placement:** done by the anneal placer, seed 1 of `a3`. Five proximity rules keep the 20 A path short (J1 → F1 → Q1 → K1 → F2 → J2), all in the bottom-right corner.
- **Routing:** done by the FreeRouting port, with its optimiser off. The optimiser took over 50 minutes on this board; routing alone takes 15 s.
  - **Router widths:** power nets route at 0.8 mm, and the nets that reach the SOT-23-6 and SO-8 pins (U1, U2, U3) route at 0.5 mm.
  - **Widening pass:** afterwards every power segment is widened to its target (V24 2 mm, the other power nets 1 mm), and kicad-cli DRC sends back any widening that breaks a clearance.
  - **Hand-drawn connections:** the router left three pin connections that I drew by hand (U1.2, U2.2, Q1.3).
- **20 A path:** carried by five F.Cu pours with solid pad connections:
  - VIN_RAW (J1 → F1)
  - VIN_F (F1 → Q1)
  - V24 (Q1 → K1)
  - V24_MOT_SW (K1 → F2)
  - V24_MOT (F2 → J2)
- **GND** is poured on B.Cu.
- **F3** (pump and valve) is 2 A, sized to the load: the pump is 10 W and the valve under 3 W, about 0.5 A. Its 0.5 mm track carries 2 A.

## Status (2026-11-03): board done

- **Checks:** ERC clean. kicad-cli DRC with zone refill and schematic parity finds 0 violations and 0 unconnected items. The tool's own placement gate still flags some reference labels sitting over a neighbour's courtyard. Silk-over-copper is clean, so this is cosmetic.
- **The board:** `board/design.json` (+ `board.json`) is the finished board, including the hand edits, the pours, the widened tracks and the label sides. It can't be rebuilt from the intent alone.
  - To open it in the studio, copy `board/` to `work/safety-power/` and set `intent` in `board.json` to the absolute path of `safety_power.yaml`.
- **Fab files** in `fab/`:
  - `safety_power-gerbers.zip`: copper, mask, paste, silk, outline and drill (the same files are in `gerbers/`).
  - `safety_power-bom-jlc.csv`: 55 lines, every part with an LCSC number.
  - `safety_power-cpl-jlc.csv` and `safety_power-pos.csv`: placement, in mm.
- **Before ordering:**
  - Choose **2 oz** copper.
  - Check the CPL rotations in JLC's preview.
  - Check DEGSON's 15EDGRC-3.81 drawing against the Phoenix MC footprint.
  - Check that the XF-507P fuse holder fits the KiCad ATO footprint.
  - The G7SA relays, JQC relays, fuse holders and connectors are through-hole: hand-solder them, or pay for JLC's THT assembly.

## Still to get or check

These are in PARTS.md, under "Still to get or measure":
- the SG7110 pinout;
- the LedWV lamp electrical details;
- the via-paste cure temperature (must be under 240 °C);
- the extractor's power and its airflow switch;
- whether the XF-507P holder matches the KiCad ATO footprint.

A held-down RESET button can re-latch the chain. This is not handled yet; an edge-triggered reset would fix it.

## Open (2026-11-03, board work paused by the user)

- **Key switch mismatch:** J15 is wired for 2 NC (both level-2 channels), but the IDEC HW1K-2CF11 in PARTS.md is 1NO+1NC, so channel B would never close. Fix: a 2 NC HW1K variant or an added contact block (verify against IDEC's datasheet). The E-stop, door, cut-out, contactor and key contact types came from the makers' pages; only the contactor, G7SA and supply datasheets are saved in `~/datasheets/pcbfab/`.
- **Two machines?** The user is considering a separate LumenPnP, run on Opulo's own open boards (CERN-OHL-W v2, `src/PCB/mobo` in opulo-inc/lumenpnp), and a laser machine. If so, J2 (motion 24 V) and J3 (pump/valve) may go. Before changing the board, decide which machine gets the hot plate and vacuum bed, whether the laser machine has motors, and whether the E-stop is shared. The galvo controller stays the JCZ LMCV4. OpenGalvo OPAL (Teensy + XY2-100, tested on an SG7110, GPL-2.0, pre-alpha, Gerbers only) is a possible experiment.
