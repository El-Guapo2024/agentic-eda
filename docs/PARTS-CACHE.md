# The parts cache

One SQLite file that knows every installed KiCad symbol and footprint: fields, pins, pads, pitch, courtyard size, whether the 3D model is found, and what
KiCad's parser refuses. The studio, the `eda` CLI and an agent ask it instead of each reading 222 MB of symbols and 15,000 footprint files.
Code: `crates/cli/src/parts/`. Design notes: ARCHITECTURE.md, "Parts cache".

- **Where**: `~/.cache/eda-studio/parts.sqlite`. Set `EDA_PARTS_DB=/some/file.sqlite` to use another file (tests and `tools/run-all-checks.sh` always do).
- **Freshness**: every query compares each library's stamp first. The CLI reads changed libraries again before it answers. The studio answers with
  what it has, names the changed ones in `"stale"`, and its indexer thread catches up. A footprint edited in place by another program shows up after
  the next deep look: the studio's start, every 5 minutes, or `eda parts refresh --deep`.
- **The first query on a machine builds the whole cache** (about 4 s).
- **kicad-cli** runs only for KiCad's verdicts: in `eda parts verify`, `refresh --verify` and the studio's indexer thread. It never runs on a query.
  Verdicts already in the library cache (`~/.cache/eda-studio/libraries/verdicts/`) are imported at once.

## `eda parts`

JSON on stdout by default; `--table` prints text. `-C <board dir>` says whose library tables to use: KiCad's global tables, plus the board's own.
The default is the current folder.

```sh
eda parts refresh                       # bring the cache up to date (--deep: every file; --verify: ask kicad-cli for verdicts; --kind symbol|footprint)
eda parts status                        # where it is, what is stale, who is building
eda parts search lm358 --table          # KiCad's chooser search, best first (same scores and order as the Symbol Chooser)
eda parts search --kind footprint soic --smd --pitch 1.27 --pins 8 --has-3d -n 20
eda parts search --kind footprint --fp-filter 'SOIC*3.9x4.9mm*' ''   # only the filter, any name
eda parts show Amplifier_Operational:LM358            # fields + every pin (unit, number, name, electrical type, shape, hidden)
eda parts show Package_SO:SOIC-8_3.9x4.9mm_P1.27mm --kind footprint --definition   # + the full editable definition
eda parts fits Device:R --table                       # the footprints the Symbol Chooser's footprint selector lists
eda parts fits Amplifier_Operational:LM358 --footprint Package_SO:SOIC-8_3.9x4.9mm_P1.27mm   # pin by pin
eda parts stats --by-lib                # counts, courtyards, 3D models, refusals, load errors
eda parts check-board -C work/myboard --table   # each part: symbol and footprint found, pins on pads, 3D model found
eda parts sql "SELECT name, pitch_nm FROM footprints WHERE kicad_wildcard('qfn-32*', name) ORDER BY pitch_nm"
eda parts verify                        # KiCad's verdict on every library not checked yet (kicad-cli, one library at a time)
```

## HTTP (a running studio)

These are answered on the connection's own thread, never behind the studio's request loop:

- `GET /api/parts/status`
- `GET /api/parts/search?kind=symbol|footprint&q=..&lib=..&pins=..&smd=1&tht=1&pitch=1.27&has3d=1&fp_filter=..&limit=..`
- `GET /api/parts/show?kind=..&id=Lib:Name&definition=1`
- `GET /api/parts/fits?symbol=Lib:Name&footprint=Lib:Name`
- `GET /api/parts/stats?by_lib=1`
- `GET /api/parts/check`: the board's parts.
- `POST /api/parts/refresh`: the indexer looks at every file of the board's libraries again.

## Tables

Lengths are nanometres; 1 mm is 1,000,000. Booleans are 0/1.

| Table | What |
|---|---|
| `libraries` | `kind`, `path`, `nickname`, `stamp`, `files_stamp`, `item_count`, `load_error`, `verdict_state` (`unknown`, `ok`, `some_refused`), `verdict_message` |
| `table_rows` | the tables' rows a refresh read: `scope` (`global` or `project:<dir>`), `kind`, `nickname`, `library_id`, `path`, `description`, `hidden` |
| `symbols` | `library_id`, `name`, `extends`, `reference`, `value`, `footprint`, `datasheet`, `description`, `keywords`, `fp_filters`, `power`, `units`, `pin_count`, `pin_numbers`, `user_fields_json`, `kicad_refused` |
| `symbol_pins` | `symbol_id`, `unit` (0 = every unit), `body_style`, `number`, `name`, `electrical_type`, `shape`, `hidden` |
| `footprints` | `library_id`, `name`, `description`, `tags`, `attr` (`smd`, `through_hole`, `unspecified`), `pads`, `numbered_pads`, `smd_pads`, `tht_pads`, `npth_pads`, `pitch_nm`, `courtyard_w_nm`, `courtyard_h_nm`, `has_courtyard`, `model_paths_json`, `model_found`, `exclude_from_bom`, `exclude_from_pos`, `kicad_refused`, `reader_error` |
| `footprint_pads` | `footprint_id`, `number`, `type`, `shape`, `x`, `y`, `rot`, `w`, `h`, `drill` |
| `symbols_fts`, `footprints_fts` | FTS5 trigram indexes (rowid = the item's id): `WHERE symbols_fts MATCH '"opamp"'` |
| `parts`, `part_params`, `part_links`, `offers` | phase 2 (supplier data): empty for now |

Definitions of the fields:

- **Pitch**: the smallest distance between the centres of two numbered copper pads, rounded to the micrometre.
- **Courtyard**: the box around `F.CrtYd`, or `B.CrtYd` when there is no front one.
- **`model_found`**: the footprint names a 3D model, and every model it names resolves the way the board's 3D view resolves it.
- **SQL function `kicad_wildcard(pattern, text)`**: KiCad's whole-text `*` / `?` match. Case does not matter.

Example queries:

```sql
-- op-amps with exactly 8 pins and a SOIC footprint filter
SELECT l.nickname || ':' || s.name FROM symbols s JOIN libraries l ON l.id = s.library_id
 WHERE s.keywords LIKE '%opamp%' AND s.pin_count = 8 AND s.fp_filters LIKE '%SOIC%';
-- the pins of a symbol, power pins first
SELECT p.unit, p.number, p.name, p.electrical_type FROM symbol_pins p JOIN symbols s ON s.id = p.symbol_id
 WHERE s.name = 'LM358' ORDER BY p.electrical_type != 'power_in', p.unit, CAST(p.number AS INT);
-- 0.5 mm pitch QFNs with a 3D model, smallest courtyard first
SELECT name, courtyard_w_nm / 1e6 AS w_mm, courtyard_h_nm / 1e6 AS h_mm FROM footprints
 WHERE pitch_nm = 500000 AND name LIKE 'QFN%' AND model_found ORDER BY courtyard_w_nm * courtyard_h_nm;
-- footprints with no courtyard, by library
SELECT l.nickname, count(*) FROM footprints f JOIN libraries l ON l.id = f.library_id WHERE NOT f.has_courtyard GROUP BY 1 ORDER BY 2 DESC;
-- symbols carrying a manufacturer part number field
SELECT name, user_fields_json FROM symbols WHERE user_fields_json LIKE '%"MPN"%' LIMIT 20;
```

## Measurements

Taken 2026-11-16 on the AWS box (16 shared cores, release build). Libraries: KiCad nightly's 223 symbol libraries and 155 footprint libraries
(378 libraries, 15,691 files): 22,911 symbols (12,326 of them derived), 806,966 pins, 15,468 footprints, 387,345 pads.

| What | Measured | Target |
|---|---|---|
| Cold full build (`eda parts refresh`, empty cache) | 3.7 s wall | < 15 s |
| Verdict pass, verdicts already in the library cache (378 libraries) | 0.15 s wall | < 5 s |
| Refresh with no change, shallow | 0 ms in the refresh (no lease taken), 20 ms process | < 5 ms |
| Refresh with no change, deep (every file stat'd) | 38 ms, 60 ms process | < 150 ms |
| One library read again | 0.12–0.14 s for the largest symbol library (MCU_ST_STM32H7, 15.5 MB); 47 ms for one footprint file of a 102-file library | < 0.5 s |
| `eda parts search lm358` | ≤ 10 ms process wall | < 100 ms |
| `eda parts fits Device:R` (140 footprints) | 30 ms | < 100 ms |
| `eda parts stats` / `stats --by-lib` | 20 ms / 50 ms | < 300 ms |
| `eda parts check-board` on mcu30 (30 parts) | 20 ms | < 300 ms |
| Studio start to the first ready Symbol Chooser search (ldo) | 755 ms with the cache, 1,010 ms without; footprint search 537 ms vs 784 ms | phase 1b |
| `/api/version` during the indexer's cold build | p50 1.1 ms, p95 1.9 ms, max 4.3 ms (afterwards: p95 1.3 ms) | unaffected |
| Database size | 124 MB | 80–150 MB |

A cold build in the studio is ready about 4.1 s after `eda board serve` starts.
