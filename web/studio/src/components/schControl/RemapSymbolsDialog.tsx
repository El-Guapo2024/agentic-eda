// Remap Legacy Library Symbols... (`eeschema.EditorControl.remapSymbols`, `SCH_EDITOR_CONTROL::RemapSymbols` -> `DIALOG_SYMBOL_REMAP`, eeschema/dialogs/
// dialog_symbol_remap.cpp at KiCad 8303b2ad): KiCad's explanation, "Remap Symbols" and the report. The server backs the project up into `rescue-backup/`, makes
// the project's symbol library table again from the legacy libraries and names each symbol's library (crates/cli/src/rescue_api.rs); the change of the
// symbols' links is one undoable command. KiCad offers the action only for a schematic none of whose symbols names a library (`remapSymbolsCondition`,
// `HasNoFullyDefinedLibIds`); here the dialog asks the server, and says so for any other schematic. Once a remap ran, the button is disabled (`m_remapped`).
import { useEffect, useState } from "react";
import { fetchRescueCandidates, remapSymbols, type RemapLine } from "../../api/rescueClient";
import { REMAP_INFO } from "../../kicad-port/rescueSymbols";
import { useStudioApi } from "../../state/store";
import { SchDialogFrame } from "./SchDialogFrame";

const COLORS: Record<RemapLine["severity"], string | undefined> = { info: undefined, warning: "var(--chrome-warning, #b80)", action: "var(--chrome-accent, #3a7)" };

export function RemapSymbolsDialog({ onClose }: { onClose: () => void }) {
  const api = useStudioApi();
  const [legacy, setLegacy] = useState<boolean | null>(null);
  const [report, setReport] = useState<RemapLine[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [remapped, setRemapped] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    fetchRescueCandidates().then(
      (r) => alive && setLegacy(r.legacy),
      (e: unknown) => alive && setError(e instanceof Error ? e.message : String(e))
    );
    return () => {
      alive = false;
    };
  }, []);

  const run = async () => {
    setBusy(true);
    try {
      const r = await remapSymbols();
      setReport(r.report);
      if (r.cmd) await api.cmd(r.cmd);
      setRemapped(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <SchDialogFrame title="Remap Symbols" width={720} onClose={onClose}>
      <div data-remap-dialog style={{ display: "flex", flexDirection: "column", gap: 8, fontSize: 12 }}>
        <div>{REMAP_INFO}</div>
        {legacy === false && (
          <div data-remap-not-legacy style={{ color: "var(--chrome-text-dim)" }}>
            The remapping can only be performed on legacy projects: a symbol of this schematic already names a library of the symbol library table.
          </div>
        )}
        {error && <div style={{ color: "var(--chrome-error, #c33)" }}>{error}</div>}
        <div>
          <button className="primary" disabled={busy || remapped || legacy !== true} onClick={() => void run()} data-remap-run>
            Remap Symbols
          </button>
        </div>
        {report.length > 0 && (
          <div data-remap-report style={{ maxHeight: 260, overflow: "auto", border: "1px solid var(--chrome-border)", padding: 6, whiteSpace: "pre-wrap" }}>
            {report.map((l, i) => (
              <div key={i} style={{ color: COLORS[l.severity] }}>
                {l.text}
              </div>
            ))}
          </div>
        )}
      </div>
    </SchDialogFrame>
  );
}
