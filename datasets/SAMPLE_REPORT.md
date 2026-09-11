# Sample Report

Generated 2026-09-10. PCB samples are **real boards** pulled live from
GitHub (see CATALOG.md for provenance/licenses); the CAD samples are real
Thingi10K STL meshes (STEP/B-rep datasets are all gated or non-API — see
CATALOG.md "Family B conclusion").

## PCB sample: 12 real `.kicad_pcb` files (`datasets/samples/pcb/`)

Command used:

```
python3 hfds.py find electron-rare/kicad9plus-permissive --ext .jsonl   # discovery of real repos via metadata
# then: GitHub git/trees API per repo, raw.githubusercontent.com fetch of .kicad_pcb files
python3 stats.py samples/pcb
```

(Note: `hfds sample` targets Hugging Face repos; since no ungated HF dataset
hosts raw `.kicad_pcb` files — see CATALOG.md — the 12 PCB samples were
instead pulled directly from the 14 real GitHub repos named in the
`kicad9plus-*` dataset's own provenance metadata, via GitHub's REST API.
This is disclosed rather than silently substituted.)

### `stats.py` output

```
file                                               layers    W(mm)    H(mm)   fps   nets  tracks  vias   J*
Despairon__kicad_mic_amp__kicad_mic_amp.kicad_pcb      24     75.0     25.0    35    412     227    17    5
EmielV2002__HaptiCAD__HaptiCAD_base__HaptiCAD_base     20     90.0     90.0   108    661     210    80    5
KungfuPancake__longboi__pcb__longboi.kicad_pcb         31    358.0     40.0   297   4160    2272   454   42
Netlist-Studio__dut_hub_hw__dut_hub.kicad_pcb          22    100.0    155.0   158   3626    1918   523   25
absmach__s0__modules__compute__am-iot-gateway.kica     29        ?        ?    24    606     349    26    0
bitaxeorg__BitaxeGT__BitaxeGT.kicad_pcb                33     65.0    112.0   187   3240    1031  1374    6
bitaxeorg__bitaxeBonanza__BitaxeBonanza.kicad_pcb      33    130.0    112.0   245   4124    1567  1060    7
chipsalliance__rocket-pcb__fmc_basic_peripheral__f     33     76.5     69.0   146  10039    2564   728    8
chromalock__OpenSquib__pcb__open_squib__open_squib     29     62.0    117.0    27    372     118   117    3
git4dcc__RTB_C11__C11.kicad_pcb                        20    160.3     71.7   161   2691    1619   218   14
hlord2000__XIAO-SX1276__XIAO-SX1276.kicad_pcb          31     17.8     26.5    62    690     272    78    4
jacob-wigent__carrier-wave__hardware__carrier-wave     18        ?        ?     0      0       0     0    0

TOTAL J*-refdes footprints analyzed: 119
Within 2.0 mm of a board edge: 4/119 = 3.4%
Median distance to nearest edge: 9.20 mm
Max distance to nearest edge: 43.92 mm
```

Full per-connector distance listing: run `python3 stats.py samples/pcb` (deterministic, no randomness — re-running reproduces the same table) or see `/tmp/full_stats.txt` captured this session.

Notes on the two "?" rows: `absmach__...am-iot-gateway.kicad_pcb` and
`jacob-wigent__...carrier-wave.kicad_pcb` have no `gr_line`/`gr_rect` segments
tagged `Edge.Cuts` that `stats.py` could find at the top level (carrier-wave
in particular has **zero footprints placed at all** — it's a true
work-in-progress outline-only board, a real and informative data point about
what "real designs" actually look like mid-project, not a parsing bug).

### The edge-connector statistic the main agent needs

**Only 3.4% (4 of 119) of J*-prefixed footprints sit within 2 mm of the
board outline** across this 12-board real-world sample. The median
connector sits **9.2 mm** from the nearest edge, and the 4 exceptions that
do hug the edge within 2mm are specifically USB-C/board-edge connectors
(`hlord2000/XIAO-SX1276` J1/J2 at 1.27mm, `git4dcc/RTB_C11` J21/JP2 at
1.84mm) — i.e. connectors that are *physically* edge-mount (USB, barrel
jack) cluster near 0, while headers/pin-connectors (`JP*`, internal `J*`
headers) are routinely 4-40mm inboard.

**Implication for placement heuristics:** "connectors go on the edge" is
true only for a small, physically-edge-mount minority of J* parts — most
J* footprints in real boards are internal headers/jumpers placed for
routing convenience, not mechanical edge access. A blanket "bias J* parts
toward the board edge" heuristic would be wrong ~97% of the time in this
sample; edge-biasing should be conditioned on footprint/connector *type*
(USB-C, barrel jack, edge-card finger) rather than refdes prefix alone.

## CAD sample: 5 STL meshes (`datasets/samples/cad/raw_meshes/`)

```
69090.stl      21484 bytes
1207661.stl   104184 bytes
1037027.stl   108784 bytes
81636.stl       6529 bytes
215991.stl      2284 bytes
```

Fetched via `HfFileSystem` listing of `datasets/Thingi10K/Thingi10K/raw_meshes/`
(10,000 individually-addressable blobs, not packed in the repo's tar.gz
bundles — confirmed live) and `hf_hub_download` of 5 random files under
200KB. These are triangle meshes (not STEP/B-rep) — see CATALOG.md for why
no ungated STEP dataset was available to sample from instead.
