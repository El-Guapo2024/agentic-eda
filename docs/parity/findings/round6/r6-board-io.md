# Round 6 findings: board files, footprints and libraries (r6-board-io, 2026-10-28)

Reviewer `r6-board-io`, read-only. Code read from main's checkout at `7d772e6e`. KiCad sources: `kicad-src-8303b2ad` (`$KICAD_SRC_DIR`) and `kicad-src-25ec2335` (the box's kicad-cli 10.99.0, and the KiCad 10 items). No port was used. Scratch: `/tmp/findings-r6-board-io.1ID8`.

## How it was measured

- **Binary.** Main at `7d772e6e`, `git archive` into the scratch dir, built with its own `CARGO_TARGET_DIR`, `nice`, `CARGO_BUILD_JOBS=2` (`cargo build -p eda-cli --bin eda`). Target dir and source copy deleted afterwards; the binary is kept at `/tmp/findings-r6-board-io.1ID8/eda-bin`.
- **Round trip.** `tools/board-roundtrip/run.sh <eda> /tmp/findings-r6-board-io.1ID8/rt 4` over the 214 boards, with `EDA_KICAD_API=0`. The kicad-cli rewrites of the originals were reused from round 5's cache (same kicad-cli 10.99.0).

| | round 5 (`8a8e47f1`) | main `7d772e6e` (now) |
|---|---|---|
| lines that differ | 32301 | **32301** |
| boards identical | 15 | **15** |

No board better or worse; **every `diff.txt` is byte-identical to round 5's**. So the round-5 merges (overbars, outline-font bold, leader flip, pad layer compare, zone `net_name`, `text_frame` clamp, pre-defined sizes) change nothing on the QA corpus, as round 5 predicted (the corpus has none of those cases). The residue is round 5's table, cause for cause. `plugins/fakeboard` still fails to import (a plugin fixture, not a board).

The round trip does not see the project file: kicad-cli's `pcb upgrade` moves a board's net classes and design settings out of the `.kicad_pcb`. F1 and F2 below are both invisible to it and were found by reading the readers against KiCad's loaders, then proved with kicad-cli.

## Priority (a): the round-5 merges, checked

- **Overbars** (`e3154817`, `crates/kicad/src/overbar.rs`). `convert_to_new_overbar_notation` against 8303b2ad `common/string_utils.cpp::ConvertToNewOverbarNotation`, branch for branch: the lone `~`, `~~` -> `~`, `~~{` writing `~~{}` and continuing at the second tilde (which then meets `~{` and bails out with the old string, as KiCad's loop does), `~{` bail-out, ` `/`}`/`)` ending an overbar, the closing `}` at the end. Matches. The call sites against 8303b2ad `pcb_io_kicad_sexpr_parser.cpp`: `parseNETINFO_ITEM` (3078), `parseNet` (327, name form only), `parseNETCLASS` `T_add_net` (3161), `parsePAD` `T_net` (6252), `parseEDA_TEXT` (750, reached from `parsePCB_TEXT_effects` and `parseTextBoxContent`, and for a footprint `property` at 5202 after `SetText( pValue )`). The net class's own name is not converted by KiCad, and is not here. The version: `parseHeader`'s 20201115 for a board without `(version)`, 0 for a footprint file, raised by a footprint's own `(version)` with `std::max`. Matches. A zone's `(net_name ..)` is not converted (KiCad's `legacyNetnameFromFile` is taken raw), so a pre-20210606 zone on `~RESET~` lands on a new net `~RESET~`, as KiCad's does. End to end: round 5's scratch board (`/tmp/findings-r6-board-io.1ID8/ob`) now writes `(net 1 "~{RESET}")`, `(add_net "~{RESET}")`, the pads' `~{RESET}` and `(gr_text "~{CS}" ..)`, which is what `kicad-cli pcb upgrade` writes.
- **Outline-font bold** (`fcfc782e`, `bold_text.rs`). Against 25ec2335 `EDA_TEXT::MigrateLegacyBoldStrokeWidth` / `isStrokeFont` (`m_unresolvedFontName` during load, `FONT::IsStroke`: "Default Font" or "KiCad Font"). Matches. End to end on round 5's `new.kicad_pcb` (20261017): ours writes `0.2` for both texts in a 20250228 file; kicad-cli's upgrade of ours gives back `0.125` (stroke) and `0.2` (outline), the original's values.
- **Leader / radial flip** (`e9726153`). Against 8303b2ad `PCB_DIMENSION_BASE::Flip` -> `Mirror`: the text position mirrored first, then the angle negated, the points mirrored, `SetMirrored( !IsMirrored() )` when side-specific. The mirror flag is derived from the layer here (`pcb_items.rs:370`), and the layer flips. Matches.
- **Pad layer compare** (`62a0d4de`, `fp_compare.rs:288-305`, `layer_sets_match`). Against 8303b2ad `drc_test_provider_library_parity.cpp::padNeedsUpdate` and `boardLayersMatchWithInnerLayerExpansion` (`EXPAND_INNER_LAYERS` is `FOOTPRINT`'s default `m_stackupMode`, footprint.cpp:95): non-copper exact, outer copper exact, `missingInnerInA` fails. The library copy is flipped to the board's side in KiCad (`temp->Flip`); here both are in the footprint frame with `opposite_side` compared separately. Same verdict. Die length and fabrication property always tested. The two gaps the code comment names (no `getBoardNormalizedLayerSet` cut to the board's enabled layers; a placed footprint's pads written with the kind's default layers) are listed in GAPS 26 "Left" and not repeated.
- **Zone `net_name`** (`e3154817`, `import.rs:2617-2622`). Against 8303b2ad `parseZONE` (8693-8711): a non-empty `net_name` differing from the zone's net name moves the zone to the net of that name (`FindNet`, else a new net). Matches. One order difference, not a finding: KiCad clears the net of a non-copper zone or a rule area *before* that check (`zone_has_net`, 8687), so a legacy keepout written `(net 1) (net_name "GND")` ends on GND; ours keeps the file's net for a rule area (no clear) and clears a non-copper zone after. Same result on every file KiCad writes (`(net 0) (net_name "")`).
- **`(text_frame N)` clamp** (`32912d58`): `std::clamp( textFrame, 0, 3 )`. Matches.

## Priority (b): the `.kicad_pro` writer and reader, checked

- **Pre-defined sizes** (`8df3629b`; writer `pcb.rs:985-1008`, reader `import.rs:832`). Against 8303b2ad `board_design_settings.cpp` `track_widths` / `via_dimensions` / `diff_pair_dimensions` `PARAM_LAMBDA`s (412-520): millimetres, the leading all-zero "use net class" entry, rows skipped when `entry.empty()`, not an object, or missing a key; index 0 is the net class's (`GetCurrentTrackWidth`: `m_trackWidthIndex <= 0` is the net class). Matches.
- **Plot params** (`eda_model::plot_params`, written into the board file): `default_items` against 25ec2335 `PCB_PLOT_PARAMS::PCB_PLOT_PARAMS` and `::Format` (every default, the order, `gerberprecision` written only when not 6, `outputformat 1` = `PLOT_FORMAT::GERBER`), `gerberprecision` clamped to 5-6 as `PCB_PLOT_PARAMS_PARSER::parseInt( GBR_DEFAULT_PRECISION - 1, GBR_DEFAULT_PRECISION )`, `excludeedgelayer no` -> Edge.Cuts on all layers. Matches.
- **Net classes in the project** (`project_net_settings`, `pcb.rs:848`): `meta.version` 5 (`netSettingsSchemaVersion`), Default priority `INT_MAX`, a class's row as its priority, `netclass_patterns`. The Default class omits `microvia_*`, `diff_pair_*`, `wire_width`, `bus_width`, `line_style`, which KiCad's `readNetClass` (`NETCLASS( name, false )`) then leaves unset (-1), and `addMissingDefaults` copies into composite classes. I tried to make that visible with kicad-cli (a composite pair with 0.1 mm gap, a microvia on a Default net, an unstyled wire in `sch export svg`; `/tmp/findings-r6-board-io.1ID8/dp`, `/sch`): identical reports and plots with and without the keys, so it is not a finding. Listed so it is not tried again.
- **Rule severities** (`import.rs:1346`): against the `rule_severities` loader, `hole_near_hole` read first and overwritten by `hole_to_hole`. Matches.
- **Schema meta**: our sections without `meta` are read as current (`JSON_SETTINGS::LoadFromFile`: `filever` stays -1, no migration), and `board.design_settings.meta.version` 3 is only written with exclusions. Matches what kicad-cli 10.99 does.

## Findings

### F1. A board with legacy design settings in `(setup ..)` loses most of them: the `(defaults ..)` block (copper edge clearance, line widths, text sizes), the pre-defined sizes and the legacy Default-class tokens. kicad-cli then judges our file by other numbers. **high**

**What differs.**
- KiCad reads every legacy token of `(setup ..)` (8303b2ad `pcb_io_kicad_sexpr_parser.cpp::parseSetup`, cases `user_trace_width`, `user_via`, `user_diff_pair`, `trace_clearance`, `via_size`, `via_drill`, `uvia_size`, `uvia_drill`, `uvia_min_size`, `uvia_min_drill`, `segment_width`, `edge_width`, `mod_edge_width`, `pcb_text_width`, `pcb_text_size`, `mod_text_width`, `mod_text_size`, `pad_size`, `pad_drill`, `max_error`, `defaults`, ...), sets `m_board->m_LegacyDesignSettingsLoaded = true`, and then **ignores the project file's design settings** (`board.cpp::SetProject`: `project.m_BoardSettings->SetParent( &project, !m_LegacyDesignSettingsLoaded );`, "Set parent, which also will load the values from JSON stored in the project if we don't have legacy design settings loaded already").
- `parseDefaults` (called for `T_defaults` inside `parseSetup`) reads `edge_clearance` (`designSettings.m_CopperEdgeClearance = parseBoardUnits( T_edge_clearance ); m_board->m_LegacyCopperEdgeClearanceLoaded = true;`), `copper_line_width`, `copper_text_dims`, `courtyard_line_width`, `edge_cuts_line_width`, `silk_line_width`, `silk_text_dims`, `fab_layers_*`, `other_layers_*`, `dimension_units`, `dimension_precision`.
- `parseSetup`'s `T_user_trace_width`: `// Make room for the netclass value  if( bds.m_TrackWidthList.empty() ) bds.m_TrackWidthList.emplace_back( 0 );` then `push_back( trackWidth )`; `T_user_via` the same with `VIA_DIMENSION( 0, 0 )`; `T_user_diff_pair` pushes without a dummy.
- Ours: `import.rs:1022` (`import_legacy_setup_minimums`) reads only `trace_min`, `clearance_min`, `via_min_size`, `through_hole_min`/`via_min_drill`, `hole_to_hole_min`, `via_min_annulus`. `import.rs:604` looks for `edge_clearance` directly under `(setup ..)`, where KiCad never has it (`parseSetup` has no `T_edge_clearance` case); it is in `(setup (defaults ..))`. Nothing reads `(defaults ..)` or the `user_*` sizes (grep: no `user_trace_width` in `crates/`). The written project then says `min_copper_edge_clearance 0.5` (`pcb.rs:715`, `KICAD_EDGE_CLEARANCE_NM`) and KiCad's Text & Graphics defaults.
- Also (same root): for a legacy board that has a `.kicad_pro` beside it, `crates/cli/src/import_kicad.rs:231-237` merges the project's design settings over the board's; KiCad uses the board's and ignores the project's.
- Note for the fixer: pcbnew's GUI (not kicad-cli) also infers the edge clearance of a legacy board without `edge_clearance` from the widest Edge.Cuts line (`files.cpp:771`, `inferLegacyEdgeClearance`, `std::max( 0, edgeWidth / 2 )`); p22 F4 has it for foreign imports.

**How to see it.** QA `issue4139.kicad_pcb` (20200512, no project): `(setup .. (defaults (edge_clearance 0.01) (silk_line_width 0.12) ..))`. Our derived `intent.kicad_pro` says `min_copper_edge_clearance 0.5`, `silk_line_width 0.1`, `silk_text_thickness 0.1`. Proof with one track added 0.38 mm from the edge (`/tmp/findings-r6-board-io.1ID8/l4139/e`): kicad-cli DRC of the original: `track_dangling 2` and nothing else of copper; of ours: **`copper_edge_clearance 1`**, "Board edge clearance violation (board setup constraints edge clearance 0.5000 mm; actual 0.3816 mm)". `issue4257.kicad_pcb` (20200119): `(user_trace_width 1)` is gone; the derived project has no `track_widths`.

**Test.** `a_legacy_boards_defaults_block_is_read`: import `issue4139.kicad_pcb`; assert `board.copper_edge_clearance_nm == Some(10_000)`, `text_graphics.silk.line_width_nm == 120_000`, `text_graphics.copper.text_thickness_nm == 300_000`. `a_legacy_boards_user_sizes_are_its_predefined_sizes`: import `issue4257.kicad_pcb`; assert `routing.track_width_presets == [1 mm]`. A slow kicad-cli test: the track above reports no `copper_edge_clearance` in our file either.

**Known?** No. GAPS 2 lists "a legacy board's layer names" and `viasonmask`; p22 F4 is the foreign-import inference only.

### F2. The project's Default class keeps only four of its sizes: its microvia and differential-pair sizes are dropped, and the board file we write gives KiCad's defaults instead. **high**

**What differs.**
- `import.rs:1169` (`merge_project_net_classes`) reads the project's `Default` class for `clearance`, `track_width`, `via_diameter`, `via_drill` only; `parse_project_net_classes` (`import.rs:1066`) skips `Default` ("the board-default fields already cover this"). `microvia_diameter`, `microvia_drill`, `diff_pair_width`, `diff_pair_gap`, `diff_pair_via_gap` of the Default class are never read, so `BoardRules::default_class` stays `None` for every KiCad 7+ project (only a legacy `(net_class ..)` block sets it, `import.rs:624`).
- The writer (`pcb.rs:620`, `write_class_micro_and_pair`) then writes `(uvia_dia 0.3) (uvia_drill 0.1)` and no `diff_pair_*` for Default. kicad-cli reads this legacy block (`parseNETCLASS`: `std::make_shared<NETCLASS>( wxEmptyString )`, initialised with KiCad's defaults, 0.2 / 0.25 for the pair) and moves it into the project (`m_LegacyNetclassesLoaded`), replacing the project's own Default.
- KiCad's loader (8303b2ad `net_settings.cpp`, `readNetClass`): `if( auto value = getInPcbUnits( entry, "microvia_diameter" ) ) nc->SetuViaDiameter( *value );` and so on for every key, the Default class included.
- Effect: the studio's router and the derived files use 0.2 / 0.25 mm pair sizes and 0.3 / 0.1 mm microvias for every net of the Default class, whatever the project says.

**How to see it.** 24 QA projects have a Default class with other values (e.g. `issue17429.kicad_pro`: `diff_pair_width 0.154`, `diff_pair_gap 0.127`, `microvia_diameter 0.254`, `microvia_drill 0.4572`; `issue16658`: 0.1 / 0.06, 0.18 / 0.07; `issue18142`, `issue11814`, `issue14549`, `fill_bad`, `reverse_via`, `complex_hierarchy`, ...). Our round trip's `issue17429/imp/.kicad/intent.kicad_pcb` writes `(net_class "Default" .. (uvia_dia 0.3) (uvia_drill 0.1))` with no pair sizes.

**Test.** `the_projects_default_class_keeps_its_microvia_and_pair_sizes`: merge `issue17429.kicad_pro`; assert `board.default_class` has `diff_pair_width 154 um`, `diff_pair_gap 127 um`, `microvia_diameter 254 um`, `microvia_drill 457.2 um`; and the written board's Default block has `(uvia_dia 0.254) (uvia_drill 0.4572) (diff_pair_width 0.154) (diff_pair_gap 0.127)`.

**Known?** No.

### F3. Board Setup's Dimensions defaults never come from the project or go back to it. **medium**

**What differs.** `BOARD_DESIGN_SETTINGS` keeps the dimension defaults in the project (8303b2ad `board_design_settings.cpp`: `defaults.dimension_units` (833), `defaults.dimension_precision` (837), `defaults.dimensions.units_format`, `suppress_zeroes`, `text_position`, `keep_text_aligned`, `arrow_length`, `extension_offset`), and migrates the precision from schema 0 (`migrateSchema0to1`). Ours holds them as `DrawingsSection::dimension_settings` (`ir.rs:2580`, edited by `Cmd::SetDimensionSettings`), but `merge_project_design_rules` (`import.rs:1293-1325`) reads no `dimension*` key and `export_kicad_pro` (`pcb.rs:766-781`) writes none. So an imported project's dimension style (e.g. inches, precision 2) is replaced by ours for every new dimension, and an edit on the page reaches no KiCad file.

**How to see it.** A `.kicad_pro` with `"defaults": {"dimension_units": 0, "dimension_precision": 2, "dimensions": {"arrow_length": 2540000, ...}}`: import, draw a dimension in the studio: it is in our default units and precision.

**Test.** `the_projects_dimension_defaults_are_read_and_written`: merge such a project; assert `dimension_settings.units == Inches`, `precision == 2` (after the schema-0 migration when `meta.version` is 0: `precision += 3` for inches), and that `export_kicad_pro_for` writes them back.

**Known?** No (GAPS 3 lists the page's other leftovers).

### F4. The tuner's `.kicad_dru` reader keeps one constraint per rule and never matches `(layer outer)` / `(layer inner)`. **medium**

**What differs.**
- `crates/kicad/src/custom_rules.rs:114` (`parse_rule`): `sexpr::find(r, "constraint")`, the first `(constraint ..)` only. 8303b2ad `drc_rule_parser.cpp::parseDRC_RULE` adds every `T_constraint` to `rule->m_Constraints`, and `DRC_ENGINE::compileRules` (drc_engine.cpp:729) files each one by its type. A rule written `(rule DDR (condition ..) (constraint length (min 50mm) (max 60mm)) (constraint skew (max 0.1mm)))` (a common shape for a length-matched bus) gives the tuner its length and no skew; written the other way round, no length.
- `tune_edit.rs:344-347` matches `CustomRule::layer` with `glob_match`. KiCad's `parseLayer` (drc_rule_parser.cpp:841-848): `T_outer` -> `LSET::ExternalCuMask()`, `T_inner` -> `LSET::InternalCuMask()`, else a layer name matched as a wildcard. `(layer outer)` here matches no layer, so the rule never applies.
- Only the length tuner reads these (`tune_edit.rs:340`, the target of Tune Length/Skew); the router reads the rules elsewhere.

**How to see it.** A `.kicad_dru` with `(rule L (constraint skew (max 0.1mm)) (constraint length (min 40mm) (max 41mm)) (condition "A.NetName == 'CLK'"))`: hover CLK with the length tuner; the status box has no target (`---`). KiCad's has 40-41 mm.

**Test.** In `custom_rules.rs`: `every_constraint_of_a_rule_is_read` (two constraints -> two `CustomRule`s, or a list per rule); in `tune_edit.rs`: `a_rule_on_the_outer_layers_targets_a_track_on_f_cu`.

**Known?** No.

### F5. A footprint library file in KiCad 5's `(module ..)` form cannot be resolved for a part; KiCad loads it. **medium**

**What differs.** `crates/kicad/src/footprint_lib.rs:75` (`parse_footprint_file`, used by `resolve_library_footprints_with` at :274 for every `Library:Footprint` a part names): `.filter(|l| sexpr::tag(l) == Some("footprint")).ok_or("top-level form is not (footprint ...); not a KiCad footprint file")`. 8303b2ad `parseFOOTPRINT_unchecked` (pcb_io_kicad_sexpr_parser.cpp:4960): `wxCHECK_MSG( CurTok() == T_module || CurTok() == T_footprint, ..`. The editor's reader (`footprint_import.rs:78`) accepts both. The round-5 overbar call added at `footprint_lib.rs:73` is therefore unreachable for the files that need it most (KiCad 5 `.pretty` libraries are all `(module ..)`, pre-20210606).

**How to see it.** A project library `old.pretty/R.kicad_mod` starting `(module R (layer F.Cu) (tedit 5B307E4C) ..)`, a part with `footprint: old:R`: the CLI prints "<path>: top-level form is not (footprint ...)" and the part has no footprint.

**Test.** In `footprint_lib.rs`: `a_kicad5_module_file_is_a_footprint`: `parse_footprint_file("(module R (layer F.Cu) (pad 1 smd rect (at 0 0) (size 1 1) (layers F.Cu F.Paste F.Mask)))", "old:R")` is `Ok` with one pad.

**Known?** No.

### F6. The project's rule values outside KiCad's range are kept; KiCad replaces them with the default. **low**

`import.rs:1230-1286` takes every `rules.*` number as it is (only `max_error` is floored at 1 nm, `min_resolved_spokes` capped at 99). KiCad's `PARAM_SCALED<int>::Load` (`include/settings/parameters.h`): `if( val > m_max || val < m_min ) val = m_default;`, with the ranges of `board_design_settings.cpp` 269-336 and 1026 (e.g. `rules.max_error` 0.0001-1 mm, default `ARC_HIGH_DEF` 0.005 mm; `min_clearance` 0-25 mm; `min_copper_edge_clearance` -0.01-25 mm; `min_silk_clearance` -10-100 mm). A hand-edited `"max_error": 0` is 5 um in KiCad and 1 nm here. Also `merge_project_design_rules` returns before reading `defaults` when the project has no `rules` object (`import.rs:1223`). Test: `an_out_of_range_rule_value_reads_as_kicads_default`.

## Reuse (active code that duplicates kicad-cli or a linked C++ library)

- Nothing new. `custom_rules.rs` is a second, partial reader of `.kicad_dru` beside the rule engine port; F4 is where the two disagree with KiCad.

## Checked and found to match (do not review again)

- Everything under priorities (a) and (b) above, except F1-F3 and F6.
- `eda_model::net_pattern` (never reviewed) against 8303b2ad `eda_pattern_match.cpp`: `CTX_NETCLASS` is `EDA_PATTERN_MATCH_REGEX_ANCHORED` plus `EDA_PATTERN_MATCH_WILDCARD_ANCHORED`, a matcher added only when its pattern compiles (`AddMatcher`), `StartsWith` = a match at 0; the wildcard reading escapes `.*+?^${}()|[]/\` and maps `*`/`?`; case-sensitive. Matches for every pattern a net class uses (Tcl ARE escapes such as `\m`, `\y`, `\x41` are refused here and compile in wxRegEx: not worth a finding).
- `pcb_generated.rs` (never reviewed): the tuning pattern's properties and their `std::map` order against 25ec2335 `PCB_TUNING_PATTERN::GetProperties` (plus `PCB_GENERATOR`'s `origin`), ghost patterns dropped on write and read. Matches.
- The round trip of all 214 boards: byte-identical to round 5 (above).

## Summary

| # | Finding | Severity | Known? |
|---|---|---|---|
| F1 | Legacy `(setup ..)`: `(defaults ..)` (edge clearance, widths, text), `user_trace_width`/`user_via`/`user_diff_pair` and legacy Default-class tokens not read; `edge_clearance` looked for in the wrong list; the project merged over a legacy board's settings. Proved: `copper_edge_clearance` reported on our file only | high | no |
| F2 | Project's Default class: microvia and diff-pair sizes dropped (24 QA projects); the board file gets KiCad's defaults | high | no |
| F3 | Board Setup's Dimensions defaults neither read from nor written to the `.kicad_pro` | medium | no |
| F4 | Tuner's `.kicad_dru` reader: first `(constraint ..)` of a rule only; `(layer outer/inner)` never matches | medium | no |
| F5 | `parse_footprint_file` refuses KiCad 5 `(module ..)` library files (KiCad loads them), so the r5 overbar fix there is unreachable | medium | no |
| F6 | Out-of-range project rule values kept (KiCad resets to default); `defaults` skipped without `rules` | low | no |

Counts: **high 2** (F1, F2), **medium 3** (F3, F4, F5), **low 1** (F6).

Round trip: 32301 lines, 15 boards identical, unchanged from round 5 (every per-board diff byte-identical).
