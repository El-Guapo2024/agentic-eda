#!/usr/bin/env python3
"""Viewer-side 3D render: attach KiCad library STEP models to the exported
board and render it with kicad-cli. The Rust exporter stays model-free on
purpose: KiCad is a viewer here, not a dependency of the toolchain.

Usage: render3d.py <board.kicad_pcb> <out.png> [--side top|bottom]
Env:   KICAD_CLI (default: the macOS app bundle binary)
Models are referenced through ${KICAD9_3DMODEL_DIR}, so the patched board
opens on any KiCad 9 install. Footprints without a mapping stay bare and
are listed on stderr.
"""
import os, re, subprocess, sys, tempfile

KICAD_CLI = os.environ.get("KICAD_CLI", "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli")
MODEL_DIR = os.environ.get("KICAD9_3DMODEL_DIR", "/Applications/KiCad/KiCad.app/Contents/SharedSupport/3dmodels")

# Metric size code for the passive chip footprints.
CHIP = {"0201": "0201_0603", "0402": "0402_1005", "0603": "0603_1608", "0805": "0805_2012", "1206": "1206_3216", "1210": "1210_3225"}
# Family by reference prefix for chip passives: (library folder, file prefix).
CHIP_FAMILY = {"R": ("Resistor_SMD", "R_"), "C": ("Capacitor_SMD", "C_"), "L": ("Inductor_SMD", "L_"), "D": ("LED_SMD", "LED_"), "F": ("Fuse", "Fuse_")}
HEADER_ROT = float(os.environ.get("HEADER_ROT", "-90"))
FIXED = {
    "SOT-23": "Package_TO_SOT_SMD/SOT-23", "SOT-23-5": "Package_TO_SOT_SMD/SOT-23-5", "SOT-23-6": "Package_TO_SOT_SMD/SOT-23-6",
    "SOT-223": "Package_TO_SOT_SMD/SOT-223",
    "SOIC-8": "Package_SO/SOIC-8_3.9x4.9mm_P1.27mm", "SOIC-14": "Package_SO/SOIC-14_3.9x8.7mm_P1.27mm", "SOIC-16": "Package_SO/SOIC-16_3.9x9.9mm_P1.27mm",
    "TSSOP-8": "Package_SO/TSSOP-8_4.4x3mm_P0.65mm", "TSSOP-14": "Package_SO/TSSOP-14_4.4x5mm_P0.65mm",
    "TSSOP-16": "Package_SO/TSSOP-16_4.4x5mm_P0.65mm", "TSSOP-20": "Package_SO/TSSOP-20_4.4x6.5mm_P0.65mm",
    "MSOP-8": "Package_SO/MSOP-8_3x3mm_P0.65mm", "MSOP-10": "Package_SO/MSOP-10_3x3mm_P0.5mm",
    "SOD-123": "Diode_SMD/D_SOD-123", "SOD-323": "Diode_SMD/D_SOD-323", "SMA": "Diode_SMD/D_SMA",
}

def model_for(key: str, ref: str):
    """Returns (library/model, offset_mm, rotate_deg) or None."""
    key = key.upper()
    if key in FIXED:
        return FIXED[key], (0, 0, 0), (0, 0, 0)
    if key in CHIP:
        fam = CHIP_FAMILY.get(re.match(r"[A-Z]+", ref).group(0)[:1])
        if fam:
            return f"{fam[0]}/{fam[1]}{CHIP[key]}Metric", (0, 0, 0), (0, 0, 0)
    m = re.match(r"PINHEADER-(\d+)$", key)
    if m:
        # KiCad's header model has pin 1 at the origin and runs along +y;
        # the eda footprint is centred with pins along +x.
        n = int(m.group(1))
        return f"Connector_PinHeader_2.54mm/PinHeader_1x{n:02d}_P2.54mm_Vertical", (-(n - 1) * 1.27, 0, 0), (0, 0, HEADER_ROT)
    return None

def patch(text: str):
    out, missing, i = [], [], 0
    fp_re = re.compile(r'\t\(footprint "(?:eda:)?([^"]+)"\n')
    pos = 0
    while True:
        m = fp_re.search(text, pos)
        if not m:
            out.append(text[pos:]); break
        end = text.index("\n\t)\n", m.end())  # footprint closes at one-tab indent
        block = text[m.start():end]
        ref = re.search(r'\(property "Reference" "([^"]+)"', block).group(1)
        hit = model_for(m.group(1), ref)
        out.append(text[pos:end])
        if hit and os.path.exists(f"{MODEL_DIR}/{hit[0].split('/')[0]}.3dshapes/{hit[0].split('/')[1]}.step"):
            lib, name = hit[0].split("/"); o, r = hit[1], hit[2]
            out.append(f'\n\t\t(model "${{KICAD9_3DMODEL_DIR}}/{lib}.3dshapes/{name}.step"\n\t\t\t(offset (xyz {o[0]} {o[1]} {o[2]})) (scale (xyz 1 1 1)) (rotate (xyz {r[0]} {r[1]} {r[2]}))\n\t\t)')
        else:
            missing.append(f"{ref} {m.group(1)}")
        pos = end
    return "".join(out), missing

def main():
    src, dst = sys.argv[1], sys.argv[2]
    side = sys.argv[sys.argv.index("--side") + 1] if "--side" in sys.argv else "top"
    text, missing = patch(open(src).read())
    if missing:
        print("no 3D model for: " + ", ".join(missing), file=sys.stderr)
    with tempfile.TemporaryDirectory() as d:
        pcb = os.path.join(d, os.path.basename(src))
        open(pcb, "w").write(text)
        view = ["--rotate", "0,0,0", "--zoom", "1.3"] if os.environ.get("RENDER_FLAT") else ["--floor", "--perspective", "--rotate", "-40,0,25", "--zoom", "1.1"]
        cmd = [KICAD_CLI, "pcb", "render", "--quality", "high", "--side", side, *view, "--background", "opaque", "-w", "1600", "-h", "1000", "-o", dst, pcb]
        r = subprocess.run(cmd, capture_output=True, text=True, env={**os.environ, "KICAD9_3DMODEL_DIR": MODEL_DIR})
        if r.returncode != 0 or not os.path.exists(dst):
            sys.exit(f"kicad-cli failed: {r.stderr[-800:]}")

if __name__ == "__main__":
    main()
