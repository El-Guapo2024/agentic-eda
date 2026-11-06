// Two pages of the Preferences dialog that belong to no editor, at KiCad 8303b2ad:
//
//   * Common (`PANEL_COMMON_SETTINGS`, common/dialogs/panel_common_settings.cpp and _base): every row of KiCad's page in its place, the ones the studio honours
//     live (kicad-port/commonSettings.ts: the icon theme, the toolbar icon size, the tables' alternating rows, High Contrast Mode's dimming factor), the
//     others greyed with the reason as the tooltip. `TransferDataFromWindow` on OK; `ResetPanel` is the dialog's "Reset Common to Defaults" (KiCad's
//     `COMMON_SETTINGS` defaults; a `COMMON_SETTINGS defaultSettings; defaultSettings.ResetToDefaults()`).
//   * Maintenance (`PANEL_MAINTENANCE`, common/dialogs/panel_maintenance.cpp and _base): Reset "Don't Show Again" Dialogs (`doClearDontShowAgain`: the kept
//     `m_DoNotShowAgain` flags and the session's `KIDIALOG` answers), Reset All Dialogs to Defaults (`doClearDialogState`: that, and the dialogs' remembered
//     control values), Reset All Program Settings to Defaults (`onResetAll`: every setting, then the dialog cancelled and opened again -- here the page is
//     reloaded, since the studio's stores read their settings when it loads), each with KiCad's six-second info-bar message. The 3D cache duration and Clear
//     "Open Recent" History are greyed: the studio keeps no 3D cache files and has no file history.
import { useEffect, useState, type ReactNode } from "react";
import { getCommonOptions, setCommonOptions } from "../state/commonOptions";
import { COMMON_ROW_NOT_OFFERED, DEFAULT_COMMON_SETTINGS, TOOLBAR_ICON_SIZES, dimmingFactorFromPercentText, dimmingPercentText, type CommonSettings, type IconTheme } from "../kicad-port/commonSettings";
import { clearDoNotShowAgain } from "../kicad-port/kidialog";
import { clearRememberedAnswers } from "../kicad-port/embeddedFiles";
import { resetScaledModelsWarning } from "../kicad-port/stepScaleWarning";
import { DIALOG_STATE_STORAGE_KEYS, STUDIO_STORAGE_PREFIXES, studioStorageKeys } from "../kicad-port/maintenance";
import type { PreferencesPageHandle } from "./preferencesPage";

const heading = (text: string) => <div style={{ fontWeight: 600, margin: "10px 0 4px" }}>{text}</div>;

/** A row of KiCad's page the studio does not offer: greyed, the reason as its tooltip. */
function NotOffered({ reason, children }: { reason: string; children: ReactNode }) {
  return (
    <div className="filter-row" title={reason} aria-disabled="true" data-not-offered="true" style={{ opacity: 0.5, cursor: "not-allowed" }}>
      {children}
    </div>
  );
}

const offCheck = (label: string, checked: boolean, reason: string) => (
  <NotOffered reason={reason}>
    <label style={{ display: "flex", gap: 6, alignItems: "center", pointerEvents: "none" }}>
      <input type="checkbox" disabled checked={checked} readOnly aria-label={label} /> {label}
    </label>
  </NotOffered>
);

/** Preferences > Common. */
export function useCommonPage(open: boolean, shown: boolean): PreferencesPageHandle {
  const [draft, setDraft] = useState<CommonSettings>(getCommonOptions().common);
  // the dimming factor is typed as a percentage and read on OK (`m_highContrastCtrl->GetValue().ToDouble( &dimmingPercent )`)
  const [dimText, setDimText] = useState(dimmingPercentText(getCommonOptions().common.hicontrastDimmingFactor));

  useEffect(() => {
    if (!open) return;
    const c = getCommonOptions().common;
    setDraft(c);
    setDimText(dimmingPercentText(c.hicontrastDimmingFactor));
  }, [open]);

  const set = (patch: Partial<CommonSettings>) => setDraft((d) => ({ ...d, ...patch }));
  const radio = (name: string, label: string, checked: boolean, onChange: () => void, tip: string, testid: string) => (
    <label style={{ marginRight: 12 }} title={tip}>
      <input type="radio" name={name} checked={checked} onChange={onChange} data-testid={testid} /> {label}
    </label>
  );
  const iconTheme = (t: IconTheme, label: string, tip: string) => radio("common-icon-theme", label, draft.iconTheme === t, () => set({ iconTheme: t }), tip, `common-icon-theme-${t}`);

  const element = !shown ? null : (
    <div data-testid="common-page" style={{ display: "flex", gap: 24, flexWrap: "wrap" }}>
      <div style={{ flex: "1 1 320px", minWidth: 0 }}>
        {heading("Rendering Engine")}
        <NotOffered reason={COMMON_ROW_NOT_OFFERED.rendering}>
          <label style={{ marginRight: 12 }} title="Hardware-accelerated graphics (recommended)">
            <input type="radio" disabled checked readOnly /> Accelerated Graphics
          </label>
          <label title="Software graphics (for computers which do not support KiCad's hardware acceleration requirements)">
            <input type="radio" disabled checked={false} readOnly /> Fallback Graphics
          </label>
        </NotOffered>
        <NotOffered reason={COMMON_ROW_NOT_OFFERED.antialiasing}>
          <span style={{ width: 110 }}>Antialiasing:</span>
          <select disabled value="2" aria-label="Antialiasing">
            <option value="0">No Antialiasing</option>
            <option value="1">Fast Antialiasing</option>
            <option value="2">High Quality Antialiasing</option>
          </select>
        </NotOffered>

        {heading("Helper Applications")}
        <NotOffered reason={COMMON_ROW_NOT_OFFERED.helpers}>
          <span style={{ width: 110 }}>Text editor:</span>
          <input disabled value="" readOnly aria-label="Text editor" style={{ flex: 1 }} />
        </NotOffered>
        <NotOffered reason={COMMON_ROW_NOT_OFFERED.helpers}>
          <label style={{ marginRight: 12 }}>
            <input type="radio" disabled checked readOnly /> System default PDF viewer
          </label>
          <label>
            <input type="radio" disabled checked={false} readOnly /> Other:
          </label>
        </NotOffered>

        {heading("User Interface")}
        {offCheck("Show icons in menus", true, COMMON_ROW_NOT_OFFERED.iconsInMenus)}
        {offCheck("Show scrollbars in editors", false, COMMON_ROW_NOT_OFFERED.scrollbars)}
        {offCheck("Focus follows mouse between schematic and PCB editors", false, COMMON_ROW_NOT_OFFERED.focusFollows)}
        {offCheck("Show popup indicator when toggling settings with hotkeys", true, COMMON_ROW_NOT_OFFERED.hotkeyFeedback)}
        <label className="filter-row" title="When enabled, use a different color for every other table row">
          <input type="checkbox" data-testid="common-grid-striping" checked={draft.gridStriping} onChange={(e) => set({ gridStriping: e.target.checked })} />
          Use alternating row colors in tables
        </label>
        {offCheck("Disable custom cursors", false, COMMON_ROW_NOT_OFFERED.customCursors)}
        <div className="filter-row">
          <span style={{ width: 130 }}>Icon theme:</span>
          {iconTheme("light", "Light", "Use icons designed for light window backgrounds")}
          {iconTheme("dark", "Dark", "Use icons designed for dark window backgrounds")}
          {iconTheme("auto", "Automatic", "Automatically choose light or dark icons based on the system color theme")}
        </div>
        <div className="filter-row">
          <span style={{ width: 130 }}>Toolbar icon size:</span>
          {TOOLBAR_ICON_SIZES.map((s) => radio("common-icon-size", s.label, draft.toolbarIconSize === s.size, () => set({ toolbarIconSize: s.size }), s.tip, `common-icon-size-${s.size}`))}
        </div>
        <div className="filter-row">
          <span style={{ width: 240 }}>High-contrast mode dimming factor:</span>
          <input aria-label="High-contrast mode dimming factor" data-testid="common-hicontrast" style={{ width: 60 }} value={dimText} onChange={(e) => setDimText(e.target.value)} />
          <span>%</span>
        </div>
      </div>

      <div style={{ flex: "1 1 280px", minWidth: 0 }}>
        {heading("Scaling")}
        <NotOffered reason="Not offered here: the studio has no actual-size zoom for a display's pixel density to correct.">
          <span>Display PPI:</span>
          <input disabled value="" readOnly aria-label="Display PPI" style={{ width: 60 }} />
          <button disabled>Detect</button>
        </NotOffered>

        {heading("Editing")}
        {offCheck("Warp mouse to anchor of moved object", true, COMMON_ROW_NOT_OFFERED.warpMouse)}
        {offCheck("First hotkey selects tool", false, COMMON_ROW_NOT_OFFERED.immediateActions)}

        {heading("Session")}
        {offCheck("Remember open files for next project launch", false, COMMON_ROW_NOT_OFFERED.session)}
        <NotOffered reason="Not offered here: the studio keeps no list of recently opened files.">
          <span style={{ width: 130 }}>File history size:</span>
          <input disabled type="number" value={9} readOnly aria-label="File history size" style={{ width: 60 }} />
        </NotOffered>

        {heading("Project Backup")}
        {offCheck("Automatically backup projects", true, COMMON_ROW_NOT_OFFERED.backup)}
        <NotOffered reason={COMMON_ROW_NOT_OFFERED.backup}>
          <span style={{ width: 160 }}>Format:</span>
          <select disabled value="0" aria-label="Backup format">
            <option value="0">Incremental</option>
            <option value="1">Zip archive</option>
          </select>
        </NotOffered>
        <NotOffered reason={COMMON_ROW_NOT_OFFERED.backup}>
          <span style={{ width: 160 }}>Location:</span>
          <select disabled value="0" aria-label="Backup location">
            <option value="0">Project directory</option>
            <option value="1">User data directory</option>
          </select>
        </NotOffered>
        <NotOffered reason={COMMON_ROW_NOT_OFFERED.backup}>
          <span style={{ width: 160 }}>Maximum total backup size:</span>
          <input disabled type="number" readOnly aria-label="Maximum total backup size" style={{ width: 60 }} />
          <span>MB</span>
        </NotOffered>
      </div>
    </div>
  );

  return {
    element,
    commit: () => setCommonOptions({ common: { ...draft, hicontrastDimmingFactor: dimmingFactorFromPercentText(dimText) } }),
    cancel: () => {},
    reset: () => {
      setDraft(DEFAULT_COMMON_SETTINGS);
      setDimText(dimmingPercentText(DEFAULT_COMMON_SETTINGS.hicontrastDimmingFactor));
    },
    resetLabel: "Reset Common to Defaults",
  };
}

/** `PANEL_MAINTENANCE::doClearDontShowAgain`: the kept `m_DoNotShowAgain` flags (`settings->m_DoNotShowAgain = {}`) and the session's answers (`KIDIALOG::ClearDoNotShowAgainDialogs`). */
export function clearDontShowAgain(): void {
  setCommonOptions({ zoneFillWarningDismissed: false });
  resetScaledModelsWarning();
  clearDoNotShowAgain();
  clearRememberedAnswers();
}

function storageOrNull(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

/** `PANEL_MAINTENANCE::doClearDialogState`: that, and the dialogs' remembered control values (`CsInternals().m_dialogControlValues = {}`). */
export function clearDialogState(): void {
  clearDontShowAgain();
  const s = storageOrNull();
  for (const k of DIALOG_STATE_STORAGE_KEYS) {
    try {
      s?.removeItem(k);
    } catch {
      /* storage unavailable */
    }
  }
}

/** Preferences > Maintenance. `infoBar` shows the dialog's info-bar message (`dlg->GetInfoBar()->ShowMessageFor( .., 6000, wxICON_INFORMATION )`). */
export function useMaintenancePage(open: boolean, shown: boolean, infoBar: (message: string) => void): PreferencesPageHandle {
  void open;
  const element = !shown ? null : (
    <div data-testid="maintenance-page">
      <NotOffered reason="Not offered here: the studio keeps no 3D cache files (KiCad's 3D model cache folder).">
        <span>3D cache file duration:</span>
        <input disabled type="number" readOnly value={30} aria-label="3D cache file duration" style={{ width: 60 }} />
        <span>days</span>
      </NotOffered>
      <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", gap: 8, marginTop: 14 }}>
        <button disabled title="Not offered here: the studio keeps no list of recently opened files.">
          Clear &quot;Open Recent&quot; History
        </button>
        <button
          data-testid="maintenance-clear-dont-show"
          onClick={() => {
            clearDontShowAgain();
            infoBar('"Don\'t show again" dialogs reset.');
          }}
        >
          Reset &quot;Don&apos;t Show Again&quot; Dialogs
        </button>
        <button
          data-testid="maintenance-clear-dialog-state"
          onClick={() => {
            clearDialogState();
            infoBar("All dialogs reset to defaults.");
          }}
        >
          Reset All Dialogs to Defaults
        </button>
        <button
          data-testid="maintenance-reset-all"
          onClick={() => {
            clearDialogState();
            const s = storageOrNull();
            for (const k of studioStorageKeys(s, STUDIO_STORAGE_PREFIXES)) {
              try {
                s?.removeItem(k);
              } catch {
                /* storage unavailable */
              }
            }
            // `wxQueueEvent( m_parent, .. wxID_CANCEL )` then `wxID_PREFERENCES` again: the studio's stores read their settings when the page loads.
            window.location.reload();
          }}
        >
          Reset All Program Settings to Defaults
        </button>
      </div>
    </div>
  );
  return { element, commit: () => {}, cancel: () => {} };
}
