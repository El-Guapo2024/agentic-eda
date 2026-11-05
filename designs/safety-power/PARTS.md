# UV-laser PCB fab: machine parts (research of 2026-11-01)

The brief is `projects/pcbfab/ELECTRONICS-BRIEF.md` on branch `pcbfab-demo` of El-Guapo2024/cad-agent (the UV-laser version).

The user's selection rule: economical, but no cheap junk. Pick good, reliable, mass-produced parts from established makers with real datasheets. Datasheets are on the box in `~/datasheets/pcbfab/{laser,hotplate,uv-vacuum,head,safety,power,mech}/`.

**Prices** are distributor snapshots from 2026-11, quantity 1. Check them before ordering.

**Decided by the user:**
- Mains is 120 V (US).
- The hot plate is capped at 240 °C.
- Height is measured with the camera plus a line laser, not a distance sensor.

## Laser zone

| Function | Pick | Key facts | Price |
|---|---|---|---|
| UV laser | JPT Lark-355-5A | **12 V DC, under 200 W, from an external supply** (JPT manual V1.2, not mains). DB15 control port: GATE pin 6 (low = emit; **high is not a safe off**, weak UV still leaks); TRIGGER pin 7; FPS pin 8; ALARM pin 3 (TTL, high = fault). **No hardware interlock input.** | about $2.7k+ (resellers) |
| Laser supply | Mean Well RSP-320-12 | 26.7 A, PFC, 40 A inrush | $59 |
| Galvo | Sino-Galvo SG7110, UV mirrors | ±15 V, 2 A rated, about 15 A peaks (sibling RC7110 and JCZ G3). **The pinout is not published**: get it from the seller or measure it | |
| Galvo supply | 2× Mean Well RSP-320-15, wired in series (the midpoint is the galvo's 0 V) | 21.4 A per rail | 2× $59 |
| Galvo/laser controller | **BJJCZ LMCV4-DIGIT(-M)** (not the FIBER board) | 5 V 3 A. Pin 12 → GATE, pin 22 → TRIGGER, pin 20 → FPS. No interlock input. MeerK40t `balormk` drives it (USB 9588:9899; experimental) | |
| Controller 5 V | Mean Well DDR-30G-5, fed from 24 V | 6 A, isolated, DIN rail | $35 |

## UV mask cure

| Function | Pick | Key facts | Price |
|---|---|---|---|
| Lamps (3×) | LedWV TY-F15-UVLEDS-100W, 395 nm | Only a product page exists: "48 V 3 A", 72×8 mm emitter. Ask LedWV: driver inside or not? CC or CV? forward voltage, fan supply, weight | |
| Lamp drivers (3×) | Mean Well HLG-150H-48AB, set to 3.0 A | Works in CC or CV. Dimming only goes down to 10 %, so on/off has to be done on the AC side. Inrush 65 A cold. If the lamps' forward voltage is above 48 V, this driver can't hold 3 A | 3× $74 |
| Better lamp (if the budget allows) | Phoseon FireJet FJ100 Gen2, 395 nm | 48 V; 24 V **fail-off** enable; 0–10 V intensity; lamp-ready and temperature outputs | quote only |

Notes:
- 395 nm is the right band for UV-cure masks.
- Plan for at least 1.5 J/cm², and check it on test boards.

## Hot plate (120 V, capped at 240 °C)

| Function | Pick | Key facts | Price |
|---|---|---|---|
| Heater | 500 W, 150×150 mm mica heater **wound for 120 V** (built to order, e.g. Tempco or BriskHeat) | The Misumi MPHK in the brief is made only for 100 V or 200 V. On 120 V the 100 V part draws 720 W | |
| Over-temperature cut-out | **Selco ORA-500-QC** (UL) | Opens at 249–271 °C, closes at 420 ± 20 °F. Its contact sits in the safety relay's channel A, so a trip needs a manual reset | $8.75 |
| SSR | Omron G3NA-210B DC5-24 with a Y92B-N50 heatsink (6 A at 120 V) | Turns on at 4 V or more. Avoid Fotek parts (counterfeits) | $25 + $23 |
| Temperature sensor | Class A PT1000 in a 3 mm stainless probe, 400 °C rating (RS PRO 350-359), in a hole drilled into the plate edge | Lead insulation rating still to be confirmed | about $47 |
| Sensor reader | Adafruit MAX31865 #3648 (4300 Ω reference) | Klipper: `sensor_type: MAX31865`, `rtd_nominal_r: 1000`, `rtd_reference_r: 4300` | $15 |

## Vacuum bed and head

| Function | Pick | Key facts | Price |
|---|---|---|---|
| Vacuum pump | Kamoer HLVP15-CI, 24 V, brushless | At least 11 L/min, −82 kPa, 10 W, PWM speed input, 8000 h life. Use two in parallel if the via fill needs more flow | about $60–100 |
| Bed valve | SMC VT307V-5G1-02 (3-port, vacuum version) | De-energised vents the bed, which is the safe state | $30 |
| Height sensing | Picotronic LI650-4-5(8x26)20DEG-F100 red line laser, at 45°, with the top camera | 13 µm per pixel (the top camera measures 13 µm/px). Red, not blue: 405 nm would expose the mask | €53 |
| Paste dispenser | Stepper auger (Styger or 0x7d style, NEMA 11), 4–10 psi feed from a small 24 V pump, SMC V114 vent valve | Dots down to 0.25 mm, under 5 % variation. Time-pressure dispensing only works down to about 0.76 mm | about $50 |

## Safety and power

| Function | Pick | Key facts | Price |
|---|---|---|---|
| Force-guided relays (4×, on the board) | Omron G7SA-3A1B DC24 (LCSC C968857); second source Panasonic SFS3-DC24V (C425144) | EN 50205 class A. 3NO+1NC. 15 mA coil. **KiCad has no footprint: draw it from the datasheet** | $11.30 / $9.31 |
| Contactors | Schneider LC1D09BL (24 V DC coil, 2.4 W) | Mirror NC contact (IEC 60947-4-1). 25 A AC-1. Making capacity 250 A | about €41–53 each |
| Door switches (2×) | Comepi SP2K10W02 + KEY3 tongue | 2 NC, positive opening, IP65 | $24 |
| E-stop | IDEC XW1E-BV402MR | 2 NC, 40 mm mushroom | about $38 |
| Key switch | IDEC HW1K-2CF11 | Key comes out in OFF only. 1NO+1NC (NC in the chain, NO to the MCU) | $42 |
| Reset button | IDEC HW1B-M110S | 1 NO, momentary | $14 |
| 24 V supply | Mean Well RSP-500-24 (or RSP-320-24 if the measured load fits) | Estimated load is about 277 W | $105 / $59 |

Certified safety relay modules cost $112–300 new. With these parts, a discrete two-channel chain costs about $45 in relays.

## Mains budget (120 V)

The loads draw about 20 A if everything runs at once:
- hot plate 4.2 A;
- lamps 5.1 A;
- laser supply about 3 A;
- 24 V supply up to 5 A;
- galvo about 1 A;
- extractor about 2 A.

The process never runs the lamps, the hot plate and the laser together. With firmware sequencing, the peak is about 12 A, which fits a 15 A circuit.

## Still to get or measure

- The SG7110 pinout.
- The LedWV lamp's electrical details.
- The Lark's warm-up time and inrush.
- The via-paste product and its cure temperature (must be under the 240 °C cap).
- The extractor's power and its airflow switch.
- The Kamoer pump's start current and a US price.
- Whether the IDEC HW and XW NC contacts are positive-opening (check the datasheets).
- The open-via leak rate: test it with the pump.
