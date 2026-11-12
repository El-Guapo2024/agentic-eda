// Rescue Symbols... (`eeschema.EditorControl.rescueSymbols`): `SCH_EDITOR_CONTROL::RescueSymbols` -> `RESCUER::RescueProject` with `DIALOG_RESCUE_EACH`
// ("Project Rescue Helper", eeschema/dialogs/dialog_rescue_each.cpp at KiCad 8303b2ad). The server finds the candidates (crates/cli/src/rescue_api.rs);
// the dialog lists them -- Accept, Symbol Name, Action Taken, each accepted at first -- with the instances of the selected one and its cached and library
// symbols side by side. "Rescue Symbols" writes the rescue library and its table row and relinks the symbols as one undoable command; "Skip Symbol Rescue"
// keeps nothing. A schematic none of whose symbols names a library gets the legacy rescuer's candidates (`LEGACY_RESCUER`: a `<project>-rescue.lib` put
// first in the project's legacy list, crates/cli/src/legacy_rescue.rs). Run on demand, so the "Never Show Again" button is not shown (`m_askShowAgain` is false).
import { useEffect, useState } from "react";
import { fetchRescueCandidates, rescueSymbols, type RescueCandidates } from "../../api/rescueClient";
import { chosenCandidates, instancesTitle, NOTHING_RESCUED, NOTHING_TO_RESCUE, RESCUE_INFO, rescueOutcome } from "../../kicad-port/rescueSymbols";
import { useStudioApi, useStudioDispatch } from "../../state/store";
import { SymbolPreview } from "../chooser/ChooserPreview";
import { SchDialogFrame } from "./SchDialogFrame";

const cell = { padding: "2px 6px", borderBottom: "1px solid var(--chrome-border)" } as const;

export function RescueSymbolsDialog({ onClose }: { onClose: () => void }) {
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const [found, setFound] = useState<RescueCandidates | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [unchecked, setUnchecked] = useState<ReadonlySet<string>>(new Set());
  const [row, setRow] = useState(0);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    fetchRescueCandidates().then(
      (r) => {
        if (!alive) return;
        // `RescueProject( .., aRunningOnDemand = true )` with no candidate: the message box alone
        if (r.candidates.length === 0) {
          dispatch({ type: "TOAST", message: NOTHING_TO_RESCUE, kind: "info" });
          onClose();
          return;
        }
        setFound(r);
      },
      (e: unknown) => alive && setError(e instanceof Error ? e.message : String(e))
    );
    return () => {
      alive = false;
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const candidates = found?.candidates ?? [];
  const selected = candidates[row];
  const skip = () => {
    dispatch({ type: "TOAST", message: NOTHING_RESCUED, kind: "info" });
    onClose();
  };
  const submit = async () => {
    const chosen = chosenCandidates(
      candidates.map((c) => c.requested),
      unchecked
    );
    const outcome = rescueOutcome(candidates.length, chosen.length);
    if (outcome) {
      dispatch({ type: "TOAST", message: outcome, kind: "info" });
      return onClose();
    }
    setBusy(true);
    try {
      const done = await rescueSymbols(chosen);
      // the legacy rescuer's cached symbols keep their names (the rescue library is put first in the project's legacy list): no command then
      if (!done.cmd || (await api.cmd(done.cmd))) {
        dispatch({ type: "TOAST", message: `Rescued ${chosen.length} symbol${chosen.length === 1 ? "" : "s"} into ${done.library}.`, kind: "info" });
        onClose();
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <SchDialogFrame
      title="Project Rescue Helper"
      width={880}
      onClose={skip}
      footer={
        <>
          <button className="primary" disabled={busy || !found} onClick={() => void submit()} data-rescue-ok>
            Rescue Symbols
          </button>
          <button onClick={skip} data-rescue-skip>
            Skip Symbol Rescue
          </button>
        </>
      }
    >
      <div data-rescue-dialog style={{ display: "flex", flexDirection: "column", gap: 8, fontSize: 12 }}>
        {error && <div style={{ color: "var(--chrome-error, #c33)" }}>{error}</div>}
        {!found && !error && <div className="panel-empty">Looking for symbols to rescue…</div>}
        {found && (
          <>
            <div style={{ whiteSpace: "pre-wrap" }}>{RESCUE_INFO}</div>
            <div style={{ fontWeight: 600 }}>Symbols to update:</div>
            <table style={{ width: "100%", borderCollapse: "collapse" }} data-rescue-candidates>
              <thead>
                <tr style={{ textAlign: "left" }}>
                  <th style={cell}>Accept</th>
                  <th style={cell}>Symbol Name</th>
                  <th style={cell}>Action Taken</th>
                </tr>
              </thead>
              <tbody>
                {candidates.map((c, i) => (
                  <tr key={c.requested} data-requested={c.requested} onClick={() => setRow(i)} style={{ cursor: "pointer", background: i === row ? "var(--chrome-selected-bg)" : undefined, color: i === row ? "var(--chrome-selected-text)" : undefined }}>
                    <td style={{ padding: "2px 6px", textAlign: "center" }}>
                      <input
                        type="checkbox"
                        checked={!unchecked.has(c.requested)}
                        onClick={(e) => e.stopPropagation()}
                        onChange={(e) => {
                          const next = new Set(unchecked);
                          if (e.target.checked) next.delete(c.requested);
                          else next.add(c.requested);
                          setUnchecked(next);
                        }}
                        aria-label={`Accept ${c.requested}`}
                      />
                    </td>
                    <td style={{ padding: "2px 6px" }}>{c.requested}</td>
                    <td style={{ padding: "2px 6px" }}>{c.action}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {selected && (
              <>
                <div style={{ fontWeight: 600 }} data-rescue-instances-title>
                  {instancesTitle(selected.instances.length)}
                </div>
                <table style={{ width: "100%", borderCollapse: "collapse" }} data-rescue-instances>
                  <thead>
                    <tr style={{ textAlign: "left" }}>
                      <th style={cell}>Reference</th>
                      <th style={cell}>Value</th>
                    </tr>
                  </thead>
                  <tbody>
                    {selected.instances.map((x) => (
                      <tr key={x.reference}>
                        <td style={{ padding: "2px 6px" }}>{x.reference}</td>
                        <td style={{ padding: "2px 6px" }}>{x.value}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                <div style={{ display: "flex", gap: 12 }}>
                  <div>
                    <div>Cached Symbol:</div>
                    <SymbolPreview symbol={selected.cache} unit={selected.unit} bodyStyle={selected.body_style} status="" height={180} />
                  </div>
                  <div>
                    <div>Library Symbol:</div>
                    <SymbolPreview symbol={selected.lib} unit={selected.unit} bodyStyle={selected.body_style} status="" height={180} />
                  </div>
                </div>
              </>
            )}
          </>
        )}
      </div>
    </SchDialogFrame>
  );
}
