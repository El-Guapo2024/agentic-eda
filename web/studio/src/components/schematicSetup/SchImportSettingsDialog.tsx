// "Import Settings from Another Project..." -- port of KiCad 25ec2335 eeschema/dialogs/dialog_sch_import_settings.cpp (`DIALOG_SCH_IMPORT_SETTINGS`) and
// `DIALOG_SCHEMATIC_SETUP::onAuxiliaryAction` (UI-LEFT-2 BB4): the project to import from and the parts to import, KiCad's boxes in its order with "Select All".
// "Import Settings" reads the other project (`POST /api/sch/import_settings`) and hands it to the pages (`SetupImportContext`), which show it unapplied until
// each page's Apply, as KiCad's pages do until OK.
//
// Differs: a browser picks a file, not a path (`wxFileDialog` on `FILEEXT::ProjectFileWildcard()`). "Compare symbol with library settings" is not offered:
// Schematic Setup here has no Symbol Parity page.
import { useState } from "react";
import { postSchImportSettings } from "../../api/client";
import type { BoardState } from "../../api/types";
import type { SchImportParts } from "../boardSetup/fields";

/** `DIALOG_SCH_IMPORT_SETTINGS_BASE`'s boxes, in its order: the option each page answers to, and KiCad's label. */
export const SCH_IMPORT_OPTIONS: ReadonlyArray<{ key: string; label: string }> = [
  { key: "formatting", label: "Formatting preferences" },
  { key: "annotation", label: "Annotation preferences" },
  { key: "field_templates", label: "Field name templates" },
  { key: "bom_presets", label: "BOM presets" },
  { key: "bom_fmt_presets", label: "BOM format presets" },
  { key: "severities", label: "Violation severities" },
  { key: "pinmap", label: "Pin conflict map" },
  { key: "net_classes", label: "Net classes" },
  { key: "bus_aliases", label: "Bus alias definitions" },
  { key: "text_variables", label: "Text variables" },
];

export function SchImportSettingsDialog({ onCancel, onImported }: { onCancel: () => void; onImported: (board: BoardState, sch: SchImportParts, options: ReadonlySet<string>) => void }) {
  const [file, setFile] = useState<File | null>(null);
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // `OnCheckboxClicked`: Import Settings is on once a box is ticked (and a project is picked)
  const ready = !!file && chosen.size > 0;
  const toggle = (key: string, on: boolean) =>
    setChosen((s) => {
      const n = new Set(s);
      if (on) n.add(key);
      else n.delete(key);
      return n;
    });

  const run = async () => {
    if (!file) return;
    setBusy(true);
    setError(null);
    try {
      const r = await postSchImportSettings({ project: await file.text(), project_name: file.name });
      if (!r.ok || !r.board || !r.formatting || !r.setup || !r.severities || !r.pin_map) {
        setError(r.message ?? `Error importing settings from project:\nProject file ${file.name} could not be loaded.`);
        return;
      }
      onImported(r.board, { formatting: r.formatting, setup: r.setup, severities: r.severities, pin_map: r.pin_map }, chosen);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={onCancel}>
      <div className="dialog" role="dialog" aria-label="Import Settings" onClick={(e) => e.stopPropagation()} style={{ minWidth: 380 }}>
        <div className="dialog-header">
          <span>Import Settings</span>
        </div>
        <div className="dialog-body">
          <label style={{ display: "block" }}>
            Import from:{" "}
            <input type="file" aria-label="Import from" accept=".kicad_pro" onChange={(e) => setFile(e.target.files?.[0] ?? null)} />
          </label>
          <div className="bs-section">Import:</div>
          {SCH_IMPORT_OPTIONS.map((o) => (
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
          <button onClick={() => setChosen(new Set(SCH_IMPORT_OPTIONS.map((o) => o.key)))} style={{ marginRight: "auto" }}>
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
