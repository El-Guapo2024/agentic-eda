// Board Setup > Design Rules > Tuning Profiles -- port of pcbnew/dialogs/panel_setup_tuning_profiles.cpp and panel_setup_tuning_profile_info.cpp: the project's time-domain tuning
// profiles. A profile has a name, a type (single or differential pair), a target impedance, the switch for time-domain tuning, how long a via takes, a grid of how long a signal takes
// along a track of a width on each copper layer (with the layers above and below as its references) and a grid of via overrides. The checks are the panel's (a profile has a name, a
// layer is configured once); Apply sends the whole page as one undoable command (`set_tuning_profiles`).
//
// Each net class names the profile it uses (the Net Classes grid's "Tuning Profile" column in KiCad); here the page has the assignments in a table of its own, because a class
// may name a profile the project does not have -- the panel keeps the name of a deleted profile -- and that is what kicad-cli's DRC reports as `missing_tuning_profile`.
// Not ported: the transmission-line calculator behind the width and delay cells (`calculateTrackParametersForCell`).
import { useEffect, useState } from "react";
import { useStudioState } from "../../state/store";
import { PROFILE_TYPE_LABEL, addTrackRow, addViaOverride, asToPs, checkProfiles, emptyProfile, missingProfiles, profilesDraftOf, profilesToSend, psToAs, sameProfiles, type ProfilesDraft } from "../../kicad-port/setupPages";
import type { ProfileLayerEntry, ProfileViaOverride, TuningProfile, TuningProfileType } from "../../api/types";
import { Check, LengthField, NumberField, PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

const NONE = "";

export function TuningProfilesPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const units = state.units;
  const board = state.board;
  const copper = board?.layers ?? [];
  const stored = board?.board_rules?.tuning_profiles;
  const page = usePageDraft<ProfilesDraft>(board?.board_rules && stored ? profilesDraftOf(stored) : undefined);
  const apply = usePageApply();
  const [sel, setSel] = useState(0);
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const draft = page.draft;
  const check = draft ? checkProfiles(draft, copper) : null;
  const index = draft ? Math.min(sel, draft.profiles.length - 1) : -1;
  const p = draft && index >= 0 ? draft.profiles[index] : undefined;
  const missing = draft ? missingProfiles(draft) : [];

  const editProfile = (fn: (p: TuningProfile) => TuningProfile) => page.edit((d) => ({ ...d, profiles: d.profiles.map((q, k) => (k === index ? fn(q) : q)) }));
  const setEntry = (row: number, patch: Partial<ProfileLayerEntry>) => editProfile((q) => ({ ...q, layer_entries: q.layer_entries.map((e, k) => (k === row ? { ...e, ...patch } : e)) }));
  const setVia = (row: number, patch: Partial<ProfileViaOverride>) => editProfile((q) => ({ ...q, via_overrides: q.via_overrides.map((v, k) => (k === row ? { ...v, ...patch } : v)) }));

  const onApply = () => {
    if (!draft || !check) return;
    const first = Object.keys(check.errors).map(Number)[0];
    if (first !== undefined || check.page) {
      if (first !== undefined) setSel(first);
      apply.setMessage({ text: check.page ?? "Fix the profile marked in red first.", bad: true });
      return;
    }
    const send = profilesToSend(draft);
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

  const classes = draft ? ["Default", ...(board?.board_rules?.net_classes ?? []).map((c) => c.name), ...Object.keys(draft.classProfiles).filter((c) => c !== "Default" && !(board?.board_rules?.net_classes ?? []).some((n) => n.name === c))] : [];

  return (
    <PageFrame title="Tuning Profiles" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!draft || !check ? (
        <p className="bs-note">This backend does not report the project's tuning profiles; update the eda binary.</p>
      ) : (
        <>
          <p className="bs-note">
            A tuning profile says how long a signal takes along the copper of a net class, so tuning can aim at a delay and not only at a length. Delays are in picoseconds per millimetre (a via's in picoseconds). The transmission-line calculator of KiCad's grid cells is not ported: type the values.
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
              <Check checked={p.enable_time_domain_tuning} onChange={(v) => editProfile((q) => ({ ...q, enable_time_domain_tuning: v }))} title="Tune to a delay with this profile's propagation delays">
                Enable time-domain tuning
              </Check>
              <div className="bs-row">
                <span className="bs-label">Via propagation delay:</span>
                <span className="bs-control">
                  <NumberField ariaLabel="Via propagation delay" value={asToPs(p.via_propagation_delay_as)} onChange={(v) => editProfile((q) => ({ ...q, via_propagation_delay_as: v === undefined ? Number.NaN : psToAs(v) }))} />
                  <span className="bs-unit">ps</span>
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
                            <LengthField ariaLabel={`Row ${row + 1} track width`} units={units} value={e.width_um} invalid={!!error} width={70} onChange={(v) => setEntry(row, { width_um: v ?? Number.NaN })} />
                          </td>
                          {p.profile_type === "differential" && (
                            <td>
                              <LengthField ariaLabel={`Row ${row + 1} diff pair gap`} units={units} value={e.diff_pair_gap_um} invalid={!!error} width={70} onChange={(v) => setEntry(row, { diff_pair_gap_um: v ?? Number.NaN })} />
                            </td>
                          )}
                          <td>
                            <NumberField ariaLabel={`Row ${row + 1} delay`} value={asToPs(e.delay_as_per_mm)} invalid={!!error} width={70} onChange={(v) => setEntry(row, { delay_as_per_mm: v === undefined ? Number.NaN : psToAs(v) })} />
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

          <div className="bs-section">Net Classes</div>
          <p className="bs-note">The profile each net class uses. A class that names a profile this page does not have is reported by kicad-cli's DRC as a missing tuning profile.</p>
          <table className="bs-grid" aria-label="Tuning profile of each net class">
            <thead>
              <tr>
                <th>Net class</th>
                <th>Tuning profile</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {classes.map((cls) => {
                const value = draft.classProfiles[cls] ?? "";
                const isMissing = missing.some(([c]) => c === cls);
                return (
                  <tr key={cls} data-class={cls}>
                    <td>{cls}</td>
                    <td>
                      <input
                        type="text"
                        list="bs-profile-names"
                        aria-label={`Tuning profile of ${cls}`}
                        className={isMissing ? "bs-invalid" : ""}
                        value={value}
                        spellCheck={false}
                        style={{ width: 180 }}
                        onChange={(e) => page.edit((d) => ({ ...d, classProfiles: { ...d.classProfiles, [cls]: e.target.value } }))}
                      />
                    </td>
                    <td className="bs-error">{isMissing ? "No such tuning profile: kicad-cli reports it as missing." : ""}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          <datalist id="bs-profile-names">
            {draft.profiles.filter((q) => q.name !== "").map((q) => (
              <option key={q.name} value={q.name} />
            ))}
          </datalist>
          {check.page && <p className="bs-error">{check.page}</p>}
        </>
      )}
    </PageFrame>
  );
}
