// Board Setup > Design Rules > Tuning Profiles -- port of pcbnew/dialogs/panel_setup_tuning_profiles.cpp and panel_setup_tuning_profile_info.cpp: the project's time-domain tuning
// profiles. A profile has a name, a type (single or differential pair), a target impedance, the switch for time-domain tuning, how long a via takes, a grid of how long a signal takes
// along a track of a width on each copper layer (with the layers above and below as its references) and a grid of via overrides. The checks are the panel's (a profile has a name, a
// layer is configured once); Apply sends the whole page as one undoable command (`set_tuning_profiles`).
//
// Each net class names the profile it uses in the Net Classes grid's "Tuning Profile" column (`GRID_DELAY_PROFILE`), as in KiCad; this page sends those names as it
// found them. A class may name a profile the project does not have -- the panel keeps the name of a deleted profile -- and that is what kicad-cli's DRC reports as
// `missing_tuning_profile`; the page lists such classes.
// The Track width, Diff pair gap and Delay cells each have the button of KiCad's `GRID_CELL_RUN_FUNCTION_EDITOR`: it works out that cell (and the delay) from the
// profile's target impedance and the board's stackup (`calculateTrackParametersForCell`, kicad-port/tuningCalculator.ts over the transmission-line calculators of
// kicad-port/transline.ts), and shows KiCad's error box when it cannot; the cells still take a typed value. The calculator works at the profile's "Frequency"
// (its value and Hz / kHz / MHz / GHz choice, `getFrequency`; shown in the largest unit it reaches, `LoadProfile`) and puts the solder mask over an outer
// layer when "Model solder mask" is ticked; with "Net Chain Bridge Propagation"'s "Global unit delay" these are KiCad 25ec2335's `m_Frequency`,
// `m_ModelSolderMask` and `m_NetChainBridgePropagationDelay`, written to the project's tuning profiles (schema 2) and read back (UI-LEFT-3 B3-4).
import { useEffect, useState } from "react";
import { useStudioState } from "../../state/store";
import { FREQUENCY_UNITS, PROFILE_TYPE_LABEL, addTrackRow, frequencyShown, addViaOverride, asToPs, checkProfiles, emptyProfile, missingProfiles, profilesDraftOf, profilesToSend, psToAs, sameProfiles, type ProfilesDraft } from "../../kicad-port/setupPages";
import type { ProfileLayerEntry, ProfileViaOverride, TuningProfile, TuningProfileType } from "../../api/types";
import { Check, LengthField, NumberField, PageFrame, usePageApply, usePageDraft, type PageProps, useSetupImport } from "./fields";
import { defaultStackup } from "../../kicad-port/boardSetupRules";
import { calculateTrackParametersForCell, type CalculationType } from "../../kicad-port/tuningCalculator";
import { MessageDialog } from "../panels/appearance/shared";

const NONE = "";

export function TuningProfilesPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const units = state.units;
  const board = state.board;
  const copper = board?.layers ?? [];
  const stored = board?.board_rules?.tuning_profiles;
  const page = usePageDraft<ProfilesDraft>(board?.board_rules && stored ? profilesDraftOf(stored) : undefined);
  // "Import Settings from Another Board..." (`ImportSettingsFrom`, UI-LEFT-2 BB4)
  useSetupImport("tuning_profiles", (b) => (b.board_rules?.tuning_profiles ? profilesDraftOf(b.board_rules.tuning_profiles) : undefined), page.importDraft);
  const apply = usePageApply();
  const [sel, setSel] = useState(0);
  // `DisplayErrorMessage` of the calculator
  const [calcError, setCalcError] = useState<string | null>(null);
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const draft = page.draft;
  const check = draft ? checkProfiles(draft, copper) : null;
  const index = draft ? Math.min(sel, draft.profiles.length - 1) : -1;
  const p = draft && index >= 0 ? draft.profiles[index] : undefined;
  // `LoadProfile`: the frequency in the largest unit it reaches, the unit then kept as chosen while the number is typed (`m_frequencyUnits`)
  const [freqUnits, setFreqUnits] = useState<Record<number, number>>({});
  const hz = p?.frequency_hz ?? 1e9;
  const unit = freqUnits[index] ?? frequencyShown(hz).unit;
  const freq = { unit, value: hz / FREQUENCY_UNITS[unit]!.scale };
  const pickUnit = (u: number) => setFreqUnits((m) => ({ ...m, [index]: u }));
  // the classes, as the board has them now, that name a profile this page does not define
  const missing = draft ? missingProfiles({ ...draft, classProfiles: stored?.class_profiles ?? {} }) : [];

  const editProfile = (fn: (p: TuningProfile) => TuningProfile) => page.edit((d) => ({ ...d, profiles: d.profiles.map((q, k) => (k === index ? fn(q) : q)) }));
  const setEntry = (row: number, patch: Partial<ProfileLayerEntry>) => editProfile((q) => ({ ...q, layer_entries: q.layer_entries.map((e, k) => (k === row ? { ...e, ...patch } : e)) }));
  const setVia = (row: number, patch: Partial<ProfileViaOverride>) => editProfile((q) => ({ ...q, via_overrides: q.via_overrides.map((v, k) => (k === row ? { ...v, ...patch } : v)) }));

  // `GRID_CELL_RUN_FUNCTION_EDITOR`'s button: `calculateTrackParametersForCell( row, col )` over `BOARD::GetStackupOrDefault()`
  const calculate = (row: number, column: CalculationType) => {
    const e = p?.layer_entries[row];
    const rules = board?.board_rules;
    if (!p || !e) return;
    const stackup = rules?.stackup ?? defaultStackup(rules?.copper_layers ?? copper.length, rules?.board_thickness_nm ?? 1_600_000);
    const out = calculateTrackParametersForCell({ profileType: p.profile_type, targetImpedance: p.target_impedance, frequencyHz: p.frequency_hz ?? 1e9, modelSolderMask: p.model_solder_mask ?? false, copperLayers: copper, stackup, row: e, column });
    if (out.kind === "error") setCalcError(out.message);
    else if (out.kind === "ok") setEntry(row, out.patch);
  };
  const calcButton = (row: number, column: CalculationType) => (
    <button className="bs-calc" aria-label={`Row ${row + 1} calculate ${column}`} title="Calculate from the target impedance and the board stackup" onClick={() => calculate(row, column)}>
      &#x21bb;
    </button>
  );

  const onApply = () => {
    if (!draft || !check) return;
    const first = Object.keys(check.errors).map(Number)[0];
    if (first !== undefined || check.page) {
      if (first !== undefined) setSel(first);
      apply.setMessage({ text: check.page ?? "Fix the profile marked in red first.", bad: true });
      return;
    }
    // the profile each class names is the Net Classes page's: sent as the board has it
    const send = profilesToSend({ ...draft, classProfiles: { ...(stored?.class_profiles ?? {}) } });
    if (sameProfiles(send, stored)) {
      apply.setMessage({ text: "Nothing to apply.", bad: false });
      page.markClean();
      return;
    }
    void apply.run({ op: "set_tuning_profiles", settings: send }, page.markClean);
  };

  const layerSelect = (label: string, value: string, onChange: (v: string) => void, optional = false) => (
    <select aria-label={label} value={value} onChange={(e) => onChange(e.target.value)}>
      {optional && <option value={NONE}>&lt;None&gt;</option>}
      {value !== NONE && !copper.includes(value) && <option value={value}>{value}</option>}
      {copper.map((l) => (
        <option key={l} value={l}>
          {l}
        </option>
      ))}
    </select>
  );


  return (
    <PageFrame title="Tuning Profiles" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!draft || !check ? (
        <p className="bs-note">This backend does not report the project's tuning profiles; update the eda binary.</p>
      ) : (
        <>
          <p className="bs-note">
            A tuning profile says how long a signal takes along the copper of a net class, so tuning can aim at a delay and not only at a length. Delays are in picoseconds per millimetre (a via's in picoseconds). The button by a track width, gap or delay works it out from the target impedance and the board stackup at the profile's frequency; a typed value is kept too.
          </p>
          <div className="bs-toolbar">
            <select aria-label="Tuning profile" value={index >= 0 ? index : ""} disabled={draft.profiles.length === 0} onChange={(e) => setSel(Number(e.target.value))}>
              {draft.profiles.map((q, k) => (
                <option key={k} value={k}>
                  {q.name || "(unnamed)"}
                </option>
              ))}
            </select>
            <button
              onClick={() => {
                page.edit((d) => ({ ...d, profiles: [...d.profiles, emptyProfile()] }));
                setSel(draft.profiles.length);
              }}
            >
              Add Tuning Profile
            </button>
            <button
              disabled={!p}
              onClick={() => {
                page.edit((d) => ({ ...d, profiles: d.profiles.filter((_, k) => k !== index) }));
                setSel(Math.max(0, index - 1));
              }}
            >
              Remove Tuning Profile
            </button>
          </div>
          {!p ? (
            <p className="bs-note">No tuning profiles.</p>
          ) : (
            <div data-profile={index}>
              {check.errors[index] && <p className="bs-error">{check.errors[index]}</p>}
              <div className="bs-row">
                <span className="bs-label">Profile name:</span>
                <span className="bs-control">
                  <input type="text" className={check.errors[index] === "Tuning profile must have a name" ? "bs-invalid" : ""} aria-label="Profile name" value={p.name} spellCheck={false} style={{ width: 200 }} onChange={(e) => editProfile((q) => ({ ...q, name: e.target.value }))} />
                </span>
                <span />
              </div>
              <div className="bs-row">
                <span className="bs-label">Profile type:</span>
                <span className="bs-control">
                  <select
                    aria-label="Profile type"
                    value={p.profile_type}
                    onChange={(e) => editProfile((q) => ({ ...q, profile_type: e.target.value as TuningProfileType }))}
                  >
                    {(Object.keys(PROFILE_TYPE_LABEL) as TuningProfileType[]).map((t) => (
                      <option key={t} value={t}>
                        {PROFILE_TYPE_LABEL[t]}
                      </option>
                    ))}
                  </select>
                </span>
                <span />
              </div>
              <div className="bs-row">
                <span className="bs-label">Target impedance:</span>
                <span className="bs-control">
                  <NumberField ariaLabel="Target impedance" value={p.target_impedance} onChange={(v) => editProfile((q) => ({ ...q, target_impedance: v ?? Number.NaN }))} />
                  <span className="bs-unit">ohms</span>
                </span>
                <span />
              </div>
              <div className="bs-row">
                <span className="bs-label">Frequency:</span>
                <span className="bs-control">
                  {/* `m_frequency` (at most 15 characters) and `m_frequencyUnits`; `getFrequency` multiplies the two */}
                  <NumberField ariaLabel="Frequency" value={freq.value} onChange={(v) => (pickUnit(freq.unit), editProfile((q) => ({ ...q, frequency_hz: v === undefined ? Number.NaN : v * FREQUENCY_UNITS[freq.unit]!.scale })))} />
                  <select aria-label="Frequency units" value={freq.unit} onChange={(e) => (pickUnit(Number(e.target.value)), editProfile((q) => ({ ...q, frequency_hz: freq.value * FREQUENCY_UNITS[Number(e.target.value)]!.scale })))}>
                    {FREQUENCY_UNITS.map((u, k) => (
                      <option key={u.label} value={k}>
                        {u.label}
                      </option>
                    ))}
                  </select>
                </span>
                <span />
              </div>
              <Check checked={p.enable_time_domain_tuning} onChange={(v) => editProfile((q) => ({ ...q, enable_time_domain_tuning: v }))} title="Tune to a delay with this profile's propagation delays">
                Enable time domain tuning
              </Check>
              <Check checked={p.model_solder_mask ?? false} onChange={(v) => editProfile((q) => ({ ...q, model_solder_mask: v }))} title="Model the solder mask over an outer layer's tracks when calculating">
                Model solder mask
              </Check>
              <div className="bs-row">
                <span className="bs-label">Via propagation delay:</span>
                <span className="bs-control">
                  <NumberField ariaLabel="Via propagation delay" value={asToPs(p.via_propagation_delay_as)} onChange={(v) => editProfile((q) => ({ ...q, via_propagation_delay_as: v === undefined ? Number.NaN : psToAs(v) }))} />
                  <span className="bs-unit">ps</span>
                </span>
                <span />
              </div>

              <div className="bs-section">Net Chain Bridge Propagation</div>
              <div className="bs-row">
                <span className="bs-label">Global unit delay:</span>
                <span className="bs-control">
                  <NumberField ariaLabel="Net chain bridge unit delay" value={asToPs(p.net_chain_bridge_delay_as_per_mm ?? 0)} onChange={(v) => editProfile((q) => ({ ...q, net_chain_bridge_delay_as_per_mm: v === undefined ? Number.NaN : psToAs(v) }))} />
                  <span className="bs-unit">ps/mm</span>
                </span>
                <span />
              </div>

              <div className="bs-section">Track Propagation</div>
              <div className="bs-scroll-x">
                <table className="bs-grid" aria-label="Track propagation">
                  <thead>
                    <tr>
                      <th>Signal layer</th>
                      <th>Top reference</th>
                      <th>Bottom reference</th>
                      <th>Track width</th>
                      {p.profile_type === "differential" && <th>Diff pair gap</th>}
                      <th>Delay (ps/mm)</th>
                      <th />
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {p.layer_entries.map((e, row) => {
                      const error = check.rowErrors[`${index}:${row}`];
                      return (
                        <tr key={row} data-row={row}>
                          <td>{layerSelect(`Row ${row + 1} signal layer`, e.signal_layer, (v) => setEntry(row, { signal_layer: v }))}</td>
                          <td>{layerSelect(`Row ${row + 1} top reference`, e.top_reference_layer ?? NONE, (v) => setEntry(row, { top_reference_layer: v === NONE ? null : v }), true)}</td>
                          <td>{layerSelect(`Row ${row + 1} bottom reference`, e.bottom_reference_layer ?? NONE, (v) => setEntry(row, { bottom_reference_layer: v === NONE ? null : v }), true)}</td>
                          <td>
                            <LengthField ariaLabel={`Row ${row + 1} track width`} units={units} value={e.width_nm} invalid={!!error} width={70} onChange={(v) => setEntry(row, { width_nm: v ?? Number.NaN })} />
                            {calcButton(row, "width")}
                          </td>
                          {p.profile_type === "differential" && (
                            <td>
                              <LengthField ariaLabel={`Row ${row + 1} diff pair gap`} units={units} value={e.diff_pair_gap_nm} invalid={!!error} width={70} onChange={(v) => setEntry(row, { diff_pair_gap_nm: v ?? Number.NaN })} />
                              {calcButton(row, "gap")}
                            </td>
                          )}
                          <td>
                            <NumberField ariaLabel={`Row ${row + 1} delay`} value={asToPs(e.delay_as_per_mm)} invalid={!!error} width={70} onChange={(v) => setEntry(row, { delay_as_per_mm: v === undefined ? Number.NaN : psToAs(v) })} />
                            {calcButton(row, "delay")}
                          </td>
                          <td>
                            <button aria-label={`Remove row ${row + 1}`} onClick={() => editProfile((q) => ({ ...q, layer_entries: q.layer_entries.filter((_, k) => k !== row) }))}>
                              Remove
                            </button>
                          </td>
                          <td className="bs-error">{error ?? ""}</td>
                        </tr>
                      );
                    })}
                    {p.layer_entries.length === 0 && (
                      <tr>
                        <td colSpan={7} className="bs-note">
                          No layers.
                        </td>
                      </tr>
                    )}
                  </tbody>
                </table>
              </div>
              <div className="bs-toolbar">
                <button onClick={() => editProfile((q) => addTrackRow(q, copper))}>Add Layer</button>
              </div>

              <div className="bs-section">Via Overrides</div>
              <div className="bs-scroll-x">
                <table className="bs-grid" aria-label="Via overrides">
                  <thead>
                    <tr>
                      <th>Signal layer from</th>
                      <th>Signal layer to</th>
                      <th>Via layer from</th>
                      <th>Via layer to</th>
                      <th>Delay (ps)</th>
                      <th />
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {p.via_overrides.map((v, row) => {
                      const error = check.rowErrors[`v${index}:${row}`];
                      return (
                        <tr key={row} data-via-row={row}>
                          <td>{layerSelect(`Via override ${row + 1} signal layer from`, v.signal_layer_from, (x) => setVia(row, { signal_layer_from: x }))}</td>
                          <td>{layerSelect(`Via override ${row + 1} signal layer to`, v.signal_layer_to, (x) => setVia(row, { signal_layer_to: x }))}</td>
                          <td>{layerSelect(`Via override ${row + 1} via layer from`, v.via_layer_from, (x) => setVia(row, { via_layer_from: x }))}</td>
                          <td>{layerSelect(`Via override ${row + 1} via layer to`, v.via_layer_to, (x) => setVia(row, { via_layer_to: x }))}</td>
                          <td>
                            <NumberField ariaLabel={`Via override ${row + 1} delay`} value={asToPs(v.delay_as)} invalid={!!error} width={70} onChange={(x) => setVia(row, { delay_as: x === undefined ? Number.NaN : psToAs(x) })} />
                          </td>
                          <td>
                            <button aria-label={`Remove via override ${row + 1}`} onClick={() => editProfile((q) => ({ ...q, via_overrides: q.via_overrides.filter((_, k) => k !== row) }))}>
                              Remove
                            </button>
                          </td>
                          <td className="bs-error">{error ?? ""}</td>
                        </tr>
                      );
                    })}
                    {p.via_overrides.length === 0 && (
                      <tr>
                        <td colSpan={7} className="bs-note">
                          No overrides.
                        </td>
                      </tr>
                    )}
                  </tbody>
                </table>
              </div>
              <div className="bs-toolbar">
                <button onClick={() => editProfile((q) => addViaOverride(q, copper))}>Add Via Override</button>
              </div>
            </div>
          )}

          {missing.length > 0 && (
            <p className="bs-error" data-missing-profiles>
              {missing.map(([c, p]) => `${c} names ${p}`).join("; ")}: no such tuning profile, which kicad-cli's DRC reports as missing. The class's profile is chosen in Net Classes.
            </p>
          )}
          {check.page && <p className="bs-error">{check.page}</p>}
          {calcError !== null && <MessageDialog title="Error" message={calcError} onOk={() => setCalcError(null)} onCancel={() => setCalcError(null)} />}
        </>
      )}
    </PageFrame>
  );
}
