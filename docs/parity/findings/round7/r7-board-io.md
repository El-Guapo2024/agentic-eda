# Round 7 findings: board files, footprints, libraries, the project file (r7-board-io, 2026-10-28)

Reviewer `r7-board-io`, read-only. Code read from main's checkout at `f057b4ea`. KiCad: `kicad-src-8303b2ad` (`$KICAD_SRC_DIR`) and `kicad-src-25ec2335` (the box's kicad-cli 10.99.0, the KiCad 10 items). No port was used. Scratch: `/tmp/findings-r7-board-io.CwG8`.

## How it was measured

- **Binary.** Main at `f057b4ea`, `git archive` into the scratch dir, `cargo build -p eda-cli --bin eda` with its own `CARGO_TARGET_DIR`, `nice`, `CARGO_BUILD_JOBS=2`. Source copy and target dir deleted afterwards; the binary is kept at `/tmp/findings-r7-board-io.CwG8/eda-bin`.
- **Round trip.** `tools/board-roundtrip/run.sh <eda> rt 4` over the 214 QA boards with `EDA_KICAD_API=0`. The kicad-cli rewrites of the originals came from round 6's cache (same kicad-cli 10.99.0).

| | round 6 (`7d772e6e`) | main `f057b4ea` (now) |
|---|---|---|
| lines that differ | 32301 | **32301** |
| boards identical | 15 | **15** |

No board is better or worse, and **every per-board `diff.txt` is byte-identical to round 6's**. The round-6 merges do not show here, as round 6 predicted: kicad-cli's `pcb upgrade` moves the design settings out of the board file, and the corpus has no backdrill, no drill chart that kicad-cli would not rebuild, and no KiCad 5 `.pretty`. `plugins/fakeboard` still fails to import (a plugin fixture, not a board).
- **New measurements this round**, since the round trip cannot see the project file:
  - the original `.kicad_pro` of every QA project against our derived `intent.kicad_pro` and the derived board's `(net_class ..)` blocks, key by key (`bin/procmp.py`, `bin/nccmp.py`);
  - every QA project's `netclass_patterns` against every net of its board, read the way KiCad reads them and the way we do (`bin/patcmp.py`);
  - kicad-cli DRC and kicad-cli zone refills (`pcb drc --refill-zones --save-board`) of the derived files, for F1 and F2.

## Priority (a): the round-6 merges, checked

- **Legacy `(setup ..)` and `(defaults ..)`** (`0035c36c`, `import.rs` `import_legacy_setup`, `import_legacy_defaults`, `legacy_setup_design`). Checked case by case against 8303b2ad `parseSetup`, `parseDefaults` and `parseDefaultTextDims`. `LEGACY_DESIGN_SETTINGS_TOKENS` is exactly the 32 cases that set `m_LegacyDesignSettingsLoaded = true`. The other two places that set it, `aux_axis_origin` and `grid_origin`, are commented out in KiCad (`//m_board->m_LegacyDesignSettingsLoaded = true;`), and nothing else in the sexpr parser sets it. Each token goes to the field KiCad's parser writes. `edge_clearance` is read only inside `(defaults ..)`. `user_trace_width` and `user_via` keep duplicates (`m_appendToExisting` is false), and `user_diff_pair`'s first pair takes index 0. The italic and keep-upright flags start off: the `BOARD_DESIGN_SETTINGS` constructor sets `m_TextItalic` / `m_TextUpright` false for every class, while the `PARAM` default for upright is true. `dimension_precision` is taken as it is (`static_cast<DIM_PRECISION>( parseInt(..) )`, no migration in the board file). Matches. The constructor's minimums for a legacy board without them are ours too: annular width `(0.5 - 0.3) / 2` = 0.1 mm in the constructor, against the `PARAM` default of 0.5 mm. One edge case, not a finding: a `(net_class "Default" ..)` block that leaves out a key gets the `NETCLASS` constructor's value in KiCad, but here it keeps the setup's `trace_clearance` / `via_size`. KiCad 5 always writes every key.
- **SetProject** (`project_json_for_board`, `import_kicad.rs:233`). Checked against 8303b2ad `board.cpp::BOARD::SetProject` (`SetParent( &project, !m_LegacyDesignSettingsLoaded )`, and for `m_LegacyNetclassesLoaded` `SetDefaultNetclass`, `SetNetclasses`, `SetNetclassPatternAssignments`). The project's `netclass_assignments` (`m_netClassLabelAssignments`) are kept by KiCad and kept here. `merge_project_predefined_sizes` and `merge_project_dimension_defaults` do nothing on the stripped text, so the board's legacy sizes survive. Matches.
- **Default class microvia / diff pair** (`merge_project_net_classes`). Checked against `net_settings.cpp::readNetClass`. On `issue17429` the derived board's Default block now has `(uvia_dia 0.254) (uvia_drill 0.4572) (diff_pair_width 0.154) (diff_pair_gap 0.127)`. Across every QA project, every project class present in our output carries the project's eight sizes (`nccmp.py`: 0 value differences). The via gap is the documented "Differs" of PARITY-pcb §64.
- **Dimensions defaults** (`merge_project_dimension_defaults`, `project_dimension_defaults`). Checked against the `PARAM_ENUM` / `PARAM<bool>` / `PARAM<int>` loaders and ranges of 8303b2ad `board_design_settings.cpp:833-860`, and against `migrateSchema0to1` (+3 for inches, +2 for millimetres, only when both keys are integers). The result matches. The doc comment's reason does not: `BOARD_DESIGN_SETTINGS` sets `m_resetParamsIfMissing = false` (`board_design_settings.cpp:48` / 25ec2335 `:50`, "We want to leave alone parameters that aren't found in the project JSON"). So a missing key, or an out-of-range enum (`PARAM_ENUM::Load`: `else if( aResetIfMissing )`), keeps the constructor's value rather than taking the `PARAM` default. The two are the same numbers for every dimension key, so the outcome is identical. Worth a one-line comment fix, nothing more. The writer omits values that equal KiCad's: in the corpus, every "missing" dimension key on ours is KiCad's default (`procmp.py`).
- **Out-of-range project values** (F6). Every `rules.*` range and default was checked against 8303b2ad `board_design_settings.cpp:269-336, 1026-1032` and 25ec2335 (same `PARAM_SCALED::Load`: `if( val > m_max || val < m_min ) val = m_default;`). `min_via_annular_width`'s out-of-range default is 0.5 mm (`DEFAULT_VIASMINSIZE`), as ours has it. Matches.
- **`.kicad_dru` constraints** (`b4ae2230`). Every `(constraint ..)` is one `CustomRule`. `(layer outer|inner)` was checked against `drc_rule_parser.cpp::parseLayer` (`LSET::ExternalCuMask()` / `InternalCuMask()`, all 30 inner layers). Matches. Not findings: a quoted `(layer "outer")` is a DSN string in KiCad, so "Unrecognized layer 'outer'" and Rescue, while here it is the outer layers; and a layer name is matched against `ENUM_MAP<PCB_LAYER_ID>`'s names rather than the board's user names. Both are hand-written-file edge cases.
- **KiCad 5 `(module ..)` files** (`4e6cae39`). Against `parseFOOTPRINT_unchecked`'s `CurTok() == T_module || CurTok() == T_footprint`. The rest of a KiCad 5 file goes through readers that already know it: `fp_arc`'s `(start CENTRE) (end START) (angle A)` (`fp_art_io.rs:186`), `(width ..)` outside `(stroke ..)`, the model's `(at (xyz ..))` in inches (`footprint_lib.rs:129`), `*.Cu` / `*.Mask`, and the overbar version 0. Matches.
- **Drill chart backdrills, post-machining and layer names** (`d72c0ae5`). Read against 25ec2335 `drill_enumerator.cpp` (`isBackdrillSet`, `backdrillOperation`: start and end must be the span's stored layers; `enumerateVia`'s `tryBackdrill`, not plated, `HOLE_VIA_BACKDRILL`, filled and capped only from the drill's own flags; `EnumerateDrillSpans`; `DRILL_SPAN::operator<`, backdrill before plain, then the stored layers), `BOARD_STACKUP::GetDrillDepth` (direction from `aSecondLayer > aFirstLayer` on layer ids, the half item after the second layer) and `pcb_drill_chart.cpp` `spanText` / `operationText`. Then **proved independently with `kicad-cli pcb upgrade`**: the fixture `crates/ops/tests/drill_chart_backdrills.kicad_pcb` was imported with every cell text replaced by `"X"`, one command was run (`eda board text add`, which runs `refresh_drill_charts`), and the board was exported. The 100 cells we rebuilt are byte-identical to kicad-cli's (`/tmp/findings-r7-board-io.CwG8/bd`). Only one difference, below the nanometre: `GetDrillDepth` halves each sublayer of a dielectric on its own (`subThickness / 2`), while ours halves the summed thickness (`import_stackup` adds the sublayers into one `thickness_mm`), so a stub can be 1 nm off on a multi-sublayer dielectric. Not a finding. That the cells are not rebuilt on import matches KiCad too: pcbnew refreshes only in `PCB_EDIT_FRAME::OnModify` and when plotting (`pcb_edit_frame.cpp:2381`, `pcb_plotter.cpp:96`), not on load.

## Findings

### F1. A net-class pattern with `|` at its top level is matched as a whole name; KiCad's anchoring makes all but its last alternative prefixes. Nets land in the wrong class: proved with kicad-cli DRC. **high**

**What differs.**
- KiCad anchors a pattern by text, with no group around it. 8303b2ad `common/eda_pattern_match.cpp::EDA_PATTERN_MATCH_REGEX_ANCHORED::SetPattern`:
  ```
  if( !pattern.StartsWith( wxT( "^" ) ) )
      pattern = wxT( "^" ) + pattern;
  if( !pattern.EndsWith( wxT( "$" ) ) )
      pattern +=  wxT( "$" );
  ```
  The net is then in the class when the leftmost match starts at 0: `EDA_COMBINED_MATCHER::StartsWith`, `if( matcher->Find( aTerm ).start == 0 ) return true;`, called from `common/project/net_settings.cpp:758` (`if( matcher->StartsWith( aNetName ) )`). So `A|B|C` reads as `^A` or `B` or `C$`. A net is in the class when `A` or `B` matches a **prefix** of it, or when `C` matches all of it.
- Ours, `crates/model/src/net_pattern.rs:23-30` (`pattern_matches` -> `regex_matches` -> `Regex::is_match`, ":402 Whether the whole of `text` matches"), matches the whole name against the whole pattern, as if it were `^(?:A|B|C)$`. The unit test `counted_repeats_and_alternation_in_the_middle` asserts the opposite of KiCad: `no("x|y|z", "xy")` (`net_pattern.rs:560`). For KiCad that pattern takes `xy`, since `^x` matches at 0.
- Callers: `BoardRules::matching_classes` / `class_of` (`crates/model/src/lib.rs:1302-1304`) and `crates/cli/src/net_names.rs:144`. Through them the router's widths and clearances, Board Setup's "nets this pattern matches", and the `add_net` lists of the board file we write all use this matching.
- Users write patterns like this all the time (`VCC|VDD`, `/MOSI|/MISO|/CLK`, `D+|D-`). Two QA projects do: `connect.kicad_pro` has `Crystal: "/XIN|/XOUT|"`, `Link: "/Plug|/Socket|"`, `SPI: "/MOSI|/MISO|/CLK|/CS-?[0-9]+|"`, and `I2C: "I2C-.*|.*SDA|.*SCL"`, where `.*SDA` is a prefix test too. The corpus scan (`bin/patcmp.py`, Python `re` used as a stand-in for wxRegEx on these simple patterns) finds 5 nets whose class differs: `/XIN32`, `/XOUT32` (Crystal in KiCad, Default here) and `/Socket-RX`, `/Socket-TX`, `/Socket-TX-Enable` (Link in KiCad, Default here). Our import then drops Crystal and Link entirely as classes with no nets (`import.rs`: "an unused class matches nothing").

**How to see it (proved).** `/tmp/findings-r7-board-io.CwG8/np`: QA `connect`, with the project's `Crystal` class given `"clearance": 1.5`.
- `kicad-cli pcb drc` of the original: **54 `clearance`** violations, e.g. "Clearance violation (netclass 'Crystal' clearance 1.5000 mm; actual 0.8750 mm)", Track [GND] against Pad 1 [/XOUT32] of C110.
- The same on ours (`eda import-kicad` + `eda board export --kicad pos`): **none**. Our `intent.kicad_pcb` has `(add_net "/XIN32")` under `(net_class "Default" ..)`, and has no Crystal class.

**Test.** In `net_pattern.rs`: `top_level_alternatives_are_prefixes_but_the_last` asserts `pattern_matches("/XIN|/XOUT|", "/XIN32")`, `pattern_matches("x|y|z", "xy")`, `pattern_matches("a|b", "ab")` (`^a` at 0), `!pattern_matches("a|b", "ba")` (`b$` matches at 1 only), `pattern_matches("I2C-.*|.*SDA|.*SCL", "SDA_EXT")`, and `!pattern_matches("^A|B$", "XB")`. Grouped patterns are unchanged: `!pattern_matches("(x|y)", "xy")`. Change the existing `no("x|y|z", "xy")` to `yes`. Slow: the DRC above, Crystal's 54 clearances on both files.

**Fix note.** Anchor as KiCad does: put `^` before and `$` after the pattern text itself, unless it already starts with `^` / ends with `$`, and accept when a match starts at 0 (`Bol` / `Eol` nodes are already in the parser). The wildcard reading (`EDA_PATTERN_MATCH_WILDCARD_ANCHORED`) is not affected, since `|` is a literal there.

**Known?** No. Round 6 checked `net_pattern` "for every pattern a net class uses" but did not try a top-level `|`.

### F2. "Allow fillets outside zone outlines" is read from the project and edited in Board Setup, but never written to the derived `.kicad_pro`: kicad-cli fills the zones with less copper than KiCad does. **high**

**What differs.**
- `zones_allow_external_fillets` is read (`crates/kicad/src/import.rs:1621-1622`), held (`BoardRules::zones_allow_external_fillets`, `crates/model/src/lib.rs:439`) and edited on Board Setup > Constraints (`crates/model/src/rules.rs:139-188`). `export_kicad_pro` (`crates/kicad/src/pcb.rs:703-811`) writes every other `BOARD_DESIGN_SETTINGS` key we hold but not this one (grep: 0 hits in `pcb.rs`).
- KiCad: `m_params.emplace_back( new PARAM<bool>( "zones_allow_external_fillets", &m_ZoneKeepExternalFillets, false ) );` (8303b2ad `board_design_settings.cpp:1034`). The filler reads it in `zone.cpp:1470` (`keepExternalFillets = board->GetDesignSettings().m_ZoneKeepExternalFillets;`). With it false, the smoothed fill is clipped back to the outline, so a filleted concave corner loses its fillet.
- The studio's fills are kicad-cli's (`zones_refilled_by_kicad`, `crates/kicad-engine/src/lib.rs:882-884`), made from the derived project, so the studio shows, checks and exports the clipped copper.

**How to see it (proved).** 10 QA projects set it true (`connect`, `issue12609`, `issue4774`, `reverse_via`, `issue15280`, `issue14559`, `fill_bad`, `issue17967`, `issue11814`, `issue12109`). For each, `kicad-cli pcb drc --refill-zones --save-board` was run on our derived files as written (`u`) and again with only `"zones_allow_external_fillets": true` added (`u2`), and the filled area was summed (`/tmp/findings-r7-board-io.CwG8/fil2`, `bin/fillarea.py`):

| board | ours as written | with the project's setting |
|---|---|---|
| `issue12609` | 2167.3620 mm² | 2167.5817 mm² |
| `issue11814` | 3021.3692 | 3021.4434 |
| `connect` | 5488.7512 | 5488.8995 |
| `issue14559` | 19939.2143 | 19939.7214 |

The other six have no concave corner it changes. The amounts are small (the fillets), but it is copper that the original's Gerbers have and ours lack.

**Test.** In `pcb.rs`: `the_external_fillets_setting_is_written_to_the_project`. Set `model.board.zones_allow_external_fillets = true` and assert `export_kicad_pro(&model)` has `board.design_settings.zones_allow_external_fillets == true`. Then assert `merge_project_design_rules` of that text gives it back (the round trip already in `pcb.rs:3258`).

**Known?** No. r6-route-rules checked the page's label and default (`r6-route-rules.md:632`), not the writer.

### F3. Board Setup > Teardrops is neither read from the project nor written to it. **medium**

**What differs.**
- KiCad keeps the teardrop settings in the project: 8303b2ad `board_design_settings.cpp:526-660`, `PARAM_LAMBDA` `"teardrop_options"` (`td_onvia`, `td_onpthpad`, `td_onsmdpad`, `td_ontrackend`, `td_onroundshapesonly`, plus the legacy `td_allow_use_two_tracks`, `td_curve_segcount`, `td_on_pad_in_zone`) and `"teardrop_parameters"` (one entry per `td_target_name`: `td_maxlen`, `td_maxheight`, `td_length_ratio`, `td_height_ratio`, `td_curve_segcount`, `td_width_to_size_filter_ratio`, ...).
- Ours: `RoutingSection::teardrop_settings` (`crates/model/src/ir.rs:1179`, defaults `:1252`), edited by `Cmd::SetTeardropSettings` (`crates/ops/src/lib.rs:4262`). Nothing in `crates/kicad` or `crates/cli` reads or writes either key (grep `teardrop_parameters`, `td_onvia`: no hits).
- So an imported project's teardrop sizes are replaced by our defaults when the user clicks Add All Teardrops. An edit on the page also never reaches the project KiCad opens, so pcbnew's next "Add Teardrops" / zone refill uses KiCad's defaults (KiCad updates teardrops on refill via `TEARDROP_MANAGER::UpdateTeardrops`).

**How to see it.** Four QA projects have non-default round-shape parameters: `issue14549` (max length 0.5 mm), `issue18142` (length ratio 0.2), `via_stack_track_width` and `via_stack_uvia_rule` (ratios 0.3 / 0.6). Import `issue18142`: the Teardrops page shows the default 0.5 length ratio, not 0.2. Set the ratio to 0.3 and export: the derived `.kicad_pro` has no `teardrop_parameters`.

**Test.** `the_projects_teardrop_parameters_are_read_and_written`: import `issue18142.kicad_pro` and assert `teardrop_settings.best_length_ratio == 0.2`. Then assert the `td_round_shape` entry of `export_kicad_pro_for` carries it, and the `teardrop_options` targets too. Our single block collapses KiCad's three targets (PARITY-pcb §15), so the writer should repeat it for `td_round_shape` and `td_rect_shape` and keep `td_track_end` at KiCad's defaults. Off-schema keys are best kept as read.

**Known?** No. PARITY-pcb §15 lists the collapsed block and the unported options, not the project file.

### F4. A `User` paper size is neither clamped nor turned for `portrait`. **low**

`crates/kicad/src/page.rs:16` -> `crates/model/src/page.rs:135` (`PageSettings::from_args`) takes `(paper "User" w h)` as written and forces `portrait: false`. 8303b2ad `pcb_io_kicad_sexpr_parser.cpp::parsePAGE_INFO`: "Perform some controls to avoid crashes if the size is edited by hands", `width < MIN_PAGE_SIZE_MM` -> `MIN_PAGE_SIZE_MM` (25.4), `> MAX_PAGE_SIZE_PCBNEW_MM` (`48000*.0254` = 1219.2), the same for the height, then `if( token == T_portrait ) pageInfo.SetPortrait( true );`, which swaps the sizes. KiCad never writes `portrait` for a `User` page (`PAGE_INFO::Format`: `if( !IsCustom() && IsPortrait() )`), so only hand-edited files reach it. Test: `a_user_page_is_clamped_as_kicad_reads_it`: `(paper "User" 10 2000)` reads as 25.4 x 1219.2 mm.

**Known?** No.

## Reuse (active code that duplicates kicad-cli or a linked C++ library)

- Nothing new. F1 is in `eda_model::net_pattern`, a hand-written regular-expression engine that stands in for wxRegEx (ARE). The port is needed: the studio assigns classes live. But it should anchor as `EDA_PATTERN_MATCH_REGEX_ANCHORED` does, not as a full match.

## Checked and found to match (do not review again)

- Everything under priority (a) above.
- The project's `track_widths` / `via_dimensions` whose first entry is a real size (`issue5320`: `[0.127, 0.127, ..]`): KiCad keeps index 0 as the net class's (`GetCurrentTrackWidth`, the panels start at 1), and ours drops it and writes `0.0`. Same meaning.
- Project keys that differ only because ours omits them and KiCad loads its default (`procmp.py` over every QA project): `apply_defaults_to_fp_*` (all false), the dimension keys, and the obsolete `allow_microvias`, `allow_blind_buried_vias`, `min_via_annulus`, `min_aperture_clearance`, `solder_mask_clearance`, `solder_paste_*` (only `project_file.cpp`'s legacy migration and BDS schema 1->2 read them).
- `page.rs` `write_board_title_block` against `TITLE_BLOCK::Format` (only set fields, `comment N` 1-based; KiCad stops at 9 comments, which the model never has). `import_page`'s standard sizes and `portrait`.
- `board_text_io.rs` (never reviewed) against 25ec2335 `EDA_TEXT::GetEffectiveTextPenWidth`: `GetPenSizeForNormal` / `GetPenSizeForBold` of the text width (/8, /5). Ours takes the automatic width at thickness 0 where KiCad takes `<= 1` nm, and does not apply `ClampTextPenSize`. Both only affect the display, never the file. Cosmetic, not filed.
- `pcb_items_io.rs` (never reviewed): the order and lock handling of the KiCad 10 item writers; nothing of its own to compare.
- `grid_item_io.rs` (never reviewed) against 25ec2335 `format( const PCB_GRID_ITEM* )` / `parsePCB_GRID_ITEM`: type word before `extent` / `spacing`, polar's angle second numbers, `angle` only when turned, `priority` always, `tick_interval` when > 0, `affects` three words with `SetAll( false )` first. Matches. No QA board has a grid item, via stitch or drill map (checked by grep over the corpus), so none of `grid_item_io`, `via_stitch_io` or `drill_chart_io`'s maps is exercised by the round trip.
- The drill chart's refresh timing (see priority (a)).

## Convergence: what still differs in this area that a user would notice, ranked

1. **Net-class patterns with a top-level `|`** (F1): wrong class, so wrong clearance and width, in the router and in the file kicad-cli checks. New, high.
2. **Zone fills without the project's external fillets** (F2): a little copper missing at concave zone corners in every kicad-cli fill and export. New, high, but small in area.
3. **Line styles: dashed and dotted lines, arcs and rectangles written solid** (`issue12831`, `issue22267`, `api_kitchen_sink`, `connect`; GAPS 2, p7-p8 F3): a user's dashed fab or silk line turns solid in the derived Gerbers. Known, about 38 lines of the residue. Visible.
4. **A dimension's manual text position is lost** (`text_position_mode 2` read as 0, GAPS 28): a dimension whose text the user moved comes back with its text in the default place. Known. Visible.
5. **Teardrop settings not taken from or given to the project** (F3). New, medium.
6. **Custom-pad and board `gr_poly` arcs flattened to segments** (GAPS 27 / 31): same shape within the arc tolerance, but the file has hundreds of points where KiCad has arcs. Known. Barely visible.
7. **The stackup's `addsublayer` summed into one layer** (r2 F17 open): the Physical Stackup page shows one dielectric where KiCad has two. Known.
8. **The Default class's diff-pair via gap, and project-only class data** (via gap, wire / bus width, colours) do not reach kicad-cli through the legacy `(net_class ..)` blocks (PARITY-pcb §64 "Differs"). Known.
9. **Cosmetic only:** zone hatch pitch written 0.5 mm (9820 lines, display only, GAPS 2), `(type default)` vs `(type solid)` (noise), 3D model `-0`, member uuids of groups and generators, Edge.Cuts loop direction, pre-10 vias' explicit `(capping no)`, the empty `(embedded_files)`, `(legacy_teardrops yes)`. Footprint `(path)` / `(sheetname)` / `(sheetfile)` (20005 lines) are by design (batch A).

Without F1-F3, the board file itself has converged: the round trip has stayed at 32301 lines for three rounds, every remaining line has a known cause, and outside the by-design footprint links and the display-only hatch pitch fewer than 800 lines are left, mostly noise. The project file, which the round trip cannot see, is where this round's new differences are.

## Summary

| # | Finding | Severity | Known? |
|---|---|---|---|
| F1 | Net-class patterns: a top-level `A\|B\|C` is matched as a whole name; KiCad anchors `^A\|B\|C$`, so `A` and `B` are prefix matches (`StartsWith`). 5 QA nets in the wrong class; proved: 54 `clearance` on the original, 0 on ours | high | no |
| F2 | `zones_allow_external_fillets` read and edited but never written to the derived project: kicad-cli's fills lose the fillets (4 QA boards, 0.07-0.5 mm² each, proved by refill) | high | no |
| F3 | Board Setup > Teardrops not read from (`teardrop_parameters`, `teardrop_options`) nor written to the project (4 QA projects non-default) | medium | no |
| F4 | `(paper "User" w h)` not clamped to 25.4-1219.2 mm; `portrait` ignored for `User` | low | no |

Counts: **high 2** (F1, F2), **medium 1** (F3), **low 1** (F4).

Round trip: 32301 lines, 15 boards identical, unchanged from round 6 (every per-board diff byte-identical).
