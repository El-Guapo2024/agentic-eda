// "Import Settings from Another Board..." -- port of KiCad 25ec2335 pcbnew/dialogs/dialog_import_settings.cpp (`DIALOG_IMPORT_SETTINGS`) and
// `DIALOG_BOARD_SETUP::onAuxiliaryAction` (UI-LEFT-2 BB4): the board to import from and the parts to import, KiCad's boxes in its order with "Select All".
// "Import Settings" reads the other board (`POST /api/import_settings`) and hands it to the pages (`SetupImportContext`), which show it unapplied, as KiCad's
// pages do until OK.
//
// Differs: a browser picks files, not a path, so the board's project (and its custom rules file) is picked with it -- the person selects the `.kicad_pcb` and
// the `.kicad_pro` beside it (and the `.kicad_dru`) together; KiCad finds them beside the board itself.
import { useState } from "react";
import { postImportSettings } from "../../api/client";
import type { BoardState } from "../../api/types";

/** `DIALOG_IMPORT_SETTINGS_BASE`'s boxes, in its order: the option each page answers to, and KiCad's label. */
export const IMPORT_OPTIONS: ReadonlyArray<{ key: string; label: string }> = [
  { key: "layers", label: "Board layers and physical stackup" },
  { key: "mask_paste", label: "Solder mask/paste defaults" },
  { key: "hatch_offsets", label: "Zone hatched fill offsets" },
  { key: "text_graphics", label: "Text & graphics default properties" },
  { key: "formatting", label: "Text & graphics formatting" },
  { key: "constraints", label: "Design rule constraints" },
  { key: "sizes", label: "Predefined track & via dimensions" },
  { key: "zones", label: "Zone defaults" },
  { key: "teardrops", label: "Teardrop defaults" },
  { key: "tuning_patterns", label: "Length-tuning pattern defaults" },
  { key: "net_classes", label: "Net classes" },
  { key: "component_classes", label: "Component classes" },
  { key: "tuning_profiles", label: "Tuning Profiles" },
  { key: "custom_rules", label: "Custom rules" },
  { key: "severities", label: "Violation severities" },
];

export function ImportSettingsDialog({ onCancel, onImported }: { onCancel: () => void; onImported: (board: BoardState, options: ReadonlySet<string>) => void }) {
  const [files, setFiles] = useState<File[]>([]);
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const board = files.find((f) => f.name.toLowerCase().endsWith(".kicad_pcb"));
  // `OnCheckboxClicked`: the Import button is on once a box is ticked (and, here, a board is picked)
  const ready = !!board && chosen.size > 0;
  const toggle = (key: string, on: boolean) =>
    setChosen((s) => {
      const n = new Set(s);
      if (on) n.add(key);
      else n.delete(key);
      return n;
    });
  // `OnSelectAll`: every box on
  const selectAll = () => setChosen(new Set(IMPORT_OPTIONS.map((o) => o.key)));

  const run = async () => {
    if (!board) return;
    setBusy(true);
    setError(null);
    try {
      const pro = files.find((f) => f.name.toLowerCase().endsWith(".kicad_pro"));
      const dru = files.find((f) => f.name.toLowerCase().endsWith(".kicad_dru"));
      const r = await postImportSettings({ board: await board.text(), board_name: board.name, project: pro ? await pro.text() : null, rules: dru ? await dru.text() : null });
      if (!r.ok || !r.board) {
        setError(r.message ?? "Error loading board file.");
        return;
      }
      onImported(r.board, chosen);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={onCancel}>
      <div className="dialog" role="dialog" aria-label="Import Settings" onClick={(e) => e.stopPropagation()} style={{ minWidth: 420 }}>
        <div className="dialog-header">
          <span>Import Settings</span>
        </div>
        <div className="dialog-body">
          <label style={{ display: "block" }}>
            Import from:{" "}
            <input type="file" aria-label="Import from" multiple accept=".kicad_pcb,.kicad_pro,.kicad_dru" onChange={(e) => setFiles([...(e.target.files ?? [])])} />
          </label>
          <p className="bs-note">Select the board file with its project file (and its custom rules file) beside it.</p>
          <div className="bs-section">Import:</div>
          {IMPORT_OPTIONS.map((o) => (
            <label key={o.key} className="filter-row" style={{ display: "block" }}>
              <input type="checkbox" checked={chosen.has(o.key)} onChange={(e) => toggle(o.key, e.target.checked)} /> {o.label}
            </label>
          ))}
          {error && (
            <p className="bs-error" role="alert" style={{ whiteSpace: "pre-line" }}>
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={selectAll} style={{ marginRight: "auto" }}>
            Select All
          </button>
          <button onClick={onCancel}>Cancel</button>
          <button className="primary" disabled={!ready || busy} onClick={() => void run()}>
            Import Settings
          </button>
        </div>
      </div>
    </div>
  );
}
