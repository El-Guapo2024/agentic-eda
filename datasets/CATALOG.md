# Dataset Catalog

Discovered 2026-09-10 via **live** queries only: Hugging Face Hub search API
(`https://huggingface.co/api/datasets?search=...`) and the GitHub REST API
(unauthenticated). No entries below came from memory/training data — every
row was looked up this session. Licenses/gating/file-lists are as reported
by the HF API `siblings`/`tags` fields or GitHub's `git/trees` API at lookup
time.

## Family A: PCB / EDA (KiCad, Gerber, Eagle, Altium, schematics)

| Repo | License | Gated | Files | Formats | Usable? |
|---|---|---|---|---|---|
| `electron-rare/kicad9plus-permissive` (mirrored as `Ailiance-fr/kicad9plus-permissive`) | CC-BY-SA-4.0 (dataset wrapper); **per-sample** SPDX in metadata (Apache-2.0/MIT/CC0/CERN-OHL-P) | No | 1 jsonl, 98 samples | `.kicad_sch` text **embedded in JSONL** (truncated at 8KB/sample), with `metadata.repo`/`commit_sha` provenance | **Partially usable.** Real KiCad 9+ S-expression schematic text, but schematics only (no `.kicad_pcb`), pre-chunked/truncated, and packed in JSONL rather than raw files. |
| `electron-rare/kicad9plus-copyleft` | GPL-3.0/CERN-OHL-S-2.0/EUPL-1.2 per-sample | No | 1 jsonl, 31 samples | same as above | Usable but copyleft — fine to inspect/learn from, not to embed in non-copyleft generated output. |
| `electron-rare/kicad9plus-sch-corpus` | — | — (deprecated, 0 siblings) | 0 | — | **Not usable — deprecated/empty**, split into the two above. |
| `Ailiance-fr/mascarade-kicad-dataset` | CC-BY-SA-4.0 | No | 1 jsonl, small | text | Usable, small, same JSONL-of-schematic-text pattern. |
| `AbijahKaj/kicad-netlist-sft-dataset` | Apache-2.0 | No | 1 parquet | KiCad netlist text | Usable for netlists, not layouts. |
| `bshada/open-schematics` (mirrored many times, e.g. `Ju-C/open-schematics`) | CC-BY-4.0 | No | 939 parquet shards | Schematic **images**, not source files | Usable for vision tasks, **not** for geometry/placement (images only). |
| `SoccerNet/SN-PCBAS-2026` | unspecified | **gated: auto** (requires request) | 13 zip | PCB **assembly images** | **Not usable without approval** — gated, and it's an image/defect-detection dataset, not layout source. |
| Everything else matched by `pcb`, `gerber`, `eda`, `circuit board`, `footprint`, `altium`, `eagle+pcb` searches | n/a | n/a | n/a | JPEG/PNG | **Not usable** — every other HF hit for these terms is a PCB-defect-detection **image** dataset (keremberke, Mobiusi, Arshia82sbn, etc.), not a source-file dataset. `gerber`, `altium`, `eagle+pcb` returned **zero** HF datasets. |

**Conclusion for Family A: there is no ungated Hugging Face dataset of raw `.kicad_pcb` board-layout files at any real scale.** The only real KiCad source-text corpora on HF are schematic-only, small (98 + 31 + a handful of samples), and JSONL-packed.

### Real PCB layouts actually used for this task (non-HF, GitHub-direct)

The `kicad9plus-*` JSONL `metadata.repo`/`commit_sha` fields are themselves a **live, verifiable index of real GitHub repos** containing genuine KiCad 9+ projects. Fetching those repos' trees via the GitHub REST API (`GET /repos/{repo}/git/trees/{sha}?recursive=1`) turned up real `.kicad_pcb` files in 16 of 18 repos (2 x 404, and 2 of the 18 — `jaguilar/kicad`, `flaviens/kicad` — are forks of KiCad's own source tree, i.e. QA fixtures, not product designs, and were excluded from sampling):

| Repo | License (SPDX) | Real board(s) |
|---|---|---|
| KungfuPancake/longboi | Apache-2.0 | longboard remote — 297 footprints, 31 layers |
| hlord2000/XIAO-SX1276 | CERN-OHL-P-2.0 | LoRa module carrier |
| chromalock/OpenSquib | CC0-1.0 | 4 board variants (squib igniter) |
| Despairon/kicad_mic_amp | MIT | mic preamp |
| EmielV2002/HaptiCAD | Apache-2.0 | 2 boards (haptic base + motor mount) |
| absmach/s0 | Apache-2.0 | 10 board files (IoT gateway modules) |
| chipsalliance/rocket-pcb | Apache-2.0 | 4 FPGA peripheral boards |
| git4dcc/RTB_C11 | Apache-2.0 | power module |
| jacob-wigent/carrier-wave | CERN-OHL-S-2.0 | carrier board (outline only, no footprints placed yet — WIP) |
| bitaxeorg/BitaxeGT | CERN-OHL-S-2.0 | Bitcoin miner board |
| bitaxeorg/bitaxeBonanza | CERN-OHL-S-2.0 | Bitcoin miner board |
| Netlist-Studio/dut_hub_hw | CERN-OHL-S-2.0 | test/DUT hub, 155x100mm |
| coquette-keyboard/pcb-mariposa | EUPL-1.2 | keyboard PCB |
| loganrf/compactPi | CERN-OHL-S-2.0 | Raspberry Pi carrier |

463 real `.kicad_pcb` files found total across these repos (incl. the two KiCad-source QA repos, which alone contribute ~400 fixture boards — excluded from the "real design" sample). **This is the actually-usable, real PCB-layout source for this task** — recorded here rather than silently substituted without explanation, per the no-silent-fallback constraint.

### Other non-HF sources noted but not fetched (size/scope)
- **GitHub code search** (`extension:kicad_pcb`) — the richest source in principle, but the REST search-code endpoint requires authentication (`curl` without a token returned "Requires authentication"); not pursued further this session.
- **Kitspace** (kitspace.org) — hosts thousands of built KiCad/Eagle projects with Gerbers; has no bulk API, would need scraping.
- **OSHWA certified hardware directory** (oshwa.org) — metadata/links to repos, not files itself.

## Family B: 3D mechanical CAD (for future cad-agent)

| Repo | License | Gated | Files | Formats | Usable? |
|---|---|---|---|---|---|
| `Thingi10K/Thingi10K` | **Mixed** — "10 open source licenses" per-model (readme states this explicitly; no single blanket license) | No | 10,000 individually-fetchable `raw_meshes/*.stl` (+ tar.gz bundles of the same, npz, renderings) | `.stl` mesh | **Usable** — ungated, real, individually addressable via `HfFileSystem` (confirmed: listing `raw_meshes/` gives 10,000 separate blobs, not one packed tar). Mesh only, **not STEP/B-rep**. |
| `allenai/objaverse` | ODC-BY | No | 99,835 `.glb` | mesh (glTF) | Usable, ungated, real — but mesh, not STEP/B-rep, and not mechanical/engineering-CAD in nature (general 3D object scans). |
| `SadilKhan/Text2CAD` | CC-BY-NC-SA-4.0 | **gated: auto** | 43 zip + pkl/pth/json | CAD construction-sequence + renders (derived from DeepCAD) | **Not usable without approval** (gated), and non-commercial license. |
| `CADCODER/DeepCAD-CQ-Vision-Paired` | unspecified | No | 5 parquet/arrow | CadQuery **Python code** + images, not STEP | Usable for code-gen tasks, not raw geometry. |
| `neka-nat/DeepCAD-2D-DXF` | CC-BY-NC-4.0 | No | 2 files (repo nearly empty: just README + gitattributes) | claims DXF | **Not usable — effectively empty** (no data files present despite the name). |
| `harrisonxliang/Fusion360CadQueryDataset` | unspecified | No | 1 parquet + 2 arrow | CadQuery code, not native Fusion360/STEP | Usable for code-gen, not raw B-rep. |

### Large non-HF sources noted, not fetched (registration-gated, out of 2GB budget, or both)
- **ABC Dataset** (official: https://deep-geometry.github.io/abc-dataset/, not on HF) — ~1M real STEP/B-rep CAD models, Creative Commons, but distributed as large per-chunk archives (each several GB) from NYU/Yandex servers — exceeds this task's 2GB budget, not fetched.
- **Fusion 360 Gallery Dataset** (official: `AutodeskAILab/Fusion360GalleryDataset` on GitHub, not on HF) — confirmed via live GitHub API lookup: reconstruction subset genuinely ships `.step`/`.smt`/`.obj`/JSON per design (live-verified from `docs/reconstruction.md`), exactly the STEP format this task wants. However the actual data bundles are not stored in the git repo itself — only docs/code are — and Autodesk gates the data download behind a request form; not fetched.
- **DeepCAD** (official: https://github.com/ChrisWu1997/DeepCAD) — 178K CAD models as construction sequences + STEP, hosted on Google Drive, not programmatically fetchable without manual click-through.

### Conclusion for Family B
No ungated HF dataset contains raw STEP/B-rep files at the time of this search. The STEP-bearing datasets (ABC, Fusion 360 Gallery, DeepCAD) are all real and well-documented, but require either registration/request forms or multi-GB manual downloads from non-API hosts, which is out of scope for the 2GB/live-API constraints of this task. **For the actual sampling step below, `Thingi10K` mesh STL files were used as the ungated, lazily-fetchable stand-in** — flagged explicitly, not silently substituted.
