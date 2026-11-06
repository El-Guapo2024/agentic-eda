// Preferences > Maintenance (`PANEL_MAINTENANCE`, common/dialogs/panel_maintenance.cpp at KiCad 8303b2ad): what its buttons clear of the studio's own storage.
//
//   * "Reset All Dialogs to Defaults" (`doClearDialogState`): KiCad forgets every dialog's remembered control values (`COMMON_SETTINGS_INTERNALS::
//     m_dialogControlValues`, which `DIALOG_SHIM` fills for each dialog). The studio's dialogs that remember what they were left at keep it under the keys of
//     `DIALOG_STATE_STORAGE_KEYS` (Find by Properties' recent searches, the footprint chooser's filters and views, the 3D model browser's folder and filter, the
//     BOM plugin, the multichannel tool). What KiCad keeps in an editor's settings file (the DRC dialog's options, the Annotate panel, the import choices) is a
//     setting, not a dialog's control value, and stays.
//   * "Reset All Program Settings to Defaults" (`onResetAll`: `SETTINGS_MANAGER::ResetToDefaults`): every setting the studio keeps in this browser, the keys
//     under `STUDIO_STORAGE_PREFIXES`.
// Pure.

/** The dialogs' remembered values (`m_dialogControlValues`). */
export const DIALOG_STATE_STORAGE_KEYS: readonly string[] = [
  "eda.findByProperties.recent",
  "eda-studio.footprint-chooser-filters",
  "eda-studio.footprint-chooser-views",
  "eda-studio.model3d-filter",
  "eda-studio.model3d-dir",
  "eda-studio.bom-plugin",
  "eda-studio.multichannel-tool.v1",
];

/** Every key of the studio's own storage starts with one of these. */
export const STUDIO_STORAGE_PREFIXES: readonly string[] = ["eda-studio.", "eda."];

/** The keys of `storage` under `prefixes`, read before any is removed (removing while walking `key( i )` skips keys). */
export function studioStorageKeys(storage: Pick<Storage, "length" | "key"> | null | undefined, prefixes: readonly string[]): string[] {
  if (!storage) return [];
  const out: string[] = [];
  try {
    for (let i = 0; i < storage.length; i++) {
      const k = storage.key(i);
      if (k !== null && prefixes.some((p) => k.startsWith(p))) out.push(k);
    }
  } catch {
    /* storage unavailable */
  }
  return out;
}
