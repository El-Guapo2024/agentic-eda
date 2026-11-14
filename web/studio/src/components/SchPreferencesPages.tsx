// Two pages of Preferences > Schematic Editor at KiCad 8303b2ad (`EDA_BASE_FRAME::ShowPreferences`: after Toolbars come Field Name Templates, Data Sources and
// Simulator; UI-LEFT-3 C3-2):
//
//   * Field Name Templates (`PANEL_TEMPLATE_FIELDNAMES( aParent, nullptr )`, eeschema/dialogs/panel_template_fieldnames.cpp): "Global Field Name Templates", the
//     person's own (`EESCHEMA_SETTINGS::m_Drawing.field_names`), a grid of Name, Visible and URL with Add ("Untitled Field", not visible: `OnAddButtonClick`),
//     Delete, Move Up and Move Down -- the grid of Schematic Setup's project page (components/schematicSetup/FieldNameTemplatesPage.tsx), on the person's list. OK is
//     `TransferDataFromWindow`: a name with white space at an end is asked about (Remove White Space / Keep White Space) and the rows with a name are kept as
//     `AddTemplateFieldName` keeps them (PreferencesDialog.tsx). No reset (a plain `wxPanel`). Where they are offered: kicad-port/fieldNameTemplates.ts.
//   * Simulator (`PANEL_SIMULATOR_PREFERENCES`, eeschema/dialogs/panel_simulator_preferences.cpp and _base): Scroll Gestures -- the action of the vertical wheel with
//     no modifier, Ctrl, Shift and Alt, and of the horizontal one -- with Reset to Mouse Defaults and Reset to Trackpad Defaults; the page's reset is the mouse
//     defaults (`ResetPanel`). On macOS Ctrl reads "Cmd" and Alt "Option". What a simulator plot does with them: kicad-port/sim/simWheel.ts.
//
// Not here: Data Sources (`PANEL_SCH_DATA_SOURCES`, ODBC data sources a browser cannot reach).
import { useState } from "react";
import type { TemplateFieldName } from "../api/types";
import { NEW_TEMPLATE_FIELD, moveRow } from "../kicad-port/schSetup";
import { DEFAULT_SIM_WHEEL_ACTIONS, SIM_WHEEL_ACTIONS, SIM_WHEEL_HORIZONTAL, SIM_WHEEL_LABELS, TRACKPAD_SIM_WHEEL_ACTIONS, type SimWheelAction, type SimWheelActions } from "../kicad-port/sim/simWheel";
import { isMac } from "../platform";

const heading = (text: string) => <div style={{ fontWeight: 600, margin: "10px 0 4px" }}>{text}</div>;

/** Preferences > Schematic Editor > Field Name Templates: the person's own templates (`m_title`: "Global Field Name Templates"). */
export function GlobalFieldNamesPage({ rows, setRows }: { rows: TemplateFieldName[]; setRows: (rows: TemplateFieldName[]) => void }) {
  const [selected, setSelected] = useState<number | null>(null);
  const setRow = (i: number, patch: Partial<TemplateFieldName>) => setRows(rows.map((f, j) => (j === i ? { ...f, ...patch } : f)));
  return (
    <div data-testid="sch-field-name-templates">
      {heading("Global Field Name Templates")}
      <table className="bs-grid" aria-label="Global field name templates">
        <thead>
          <tr>
            <th>Name</th>
            <th>Visible</th>
            <th>URL</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((f, i) => (
            <tr key={i} className={selected === i ? "bs-sel" : undefined} onClick={() => setSelected(i)}>
              <td>
                <input aria-label={`Field name ${i + 1}`} value={f.name} spellCheck={false} onFocus={() => setSelected(i)} onChange={(e) => setRow(i, { name: e.target.value })} style={{ width: 240 }} />
              </td>
              <td style={{ textAlign: "center" }}>
                <input type="checkbox" aria-label={`Field ${i + 1} visible`} checked={f.visible} onChange={(e) => setRow(i, { visible: e.target.checked })} />
              </td>
              <td style={{ textAlign: "center" }}>
                <input type="checkbox" aria-label={`Field ${i + 1} URL`} checked={f.url} onChange={(e) => setRow(i, { url: e.target.checked })} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div style={{ display: "flex", gap: 6, marginTop: 6 }}>
        <button
          aria-label="Add field"
          title="Add field"
          data-testid="global-field-add"
          onClick={() => {
            setRows([...rows, { ...NEW_TEMPLATE_FIELD }]);
            setSelected(rows.length);
          }}
        >
          +
        </button>
        <button
          title="Move up"
          disabled={selected === null || selected <= 0}
          onClick={() => {
            if (selected === null) return;
            setRows(moveRow(rows, selected, -1));
            setSelected(selected - 1);
          }}
        >
          Up
        </button>
        <button
          title="Move down"
          disabled={selected === null || selected >= rows.length - 1}
          onClick={() => {
            if (selected === null) return;
            setRows(moveRow(rows, selected, 1));
            setSelected(selected + 1);
          }}
        >
          Down
        </button>
        <button
          aria-label="Delete field"
          title="Delete field"
          style={{ marginLeft: 12 }}
          disabled={selected === null || selected >= rows.length}
          onClick={() => {
            if (selected === null) return;
            setRows(rows.filter((_, j) => j !== selected));
            setSelected(rows.length > 1 ? Math.min(selected, rows.length - 2) : null);
          }}
        >
          Delete
        </button>
      </div>
    </div>
  );
}

/** Preferences > Schematic Editor > Simulator. */
export function SimulatorPrefsPage({ actions, setActions }: { actions: SimWheelActions; setActions: (a: SimWheelActions) => void }) {
  const mac = isMac();
  const row = (label: string, key: keyof SimWheelActions, choices: readonly SimWheelAction[]) => (
    <div className="filter-row">
      <span style={{ width: 90 }}>{label}</span>
      <select aria-label={`Scroll ${label}`} data-testid={`pref-sim-wheel-${key}`} value={choices.includes(actions[key]) ? actions[key] : "none"} onChange={(e) => setActions({ ...actions, [key]: e.target.value as SimWheelAction })}>
        {choices.map((a) => (
          <option key={a} value={a}>
            {SIM_WHEEL_LABELS[a]}
          </option>
        ))}
      </select>
    </div>
  );
  return (
    <div data-testid="sch-simulator-prefs">
      {heading("Scroll Gestures")}
      <div style={{ fontSize: 12, opacity: 0.8, margin: "4px 0" }}>Vertical Touchpad or Scroll Wheel Movement</div>
      <div className="filter-row" style={{ fontSize: 12, opacity: 0.75 }}>
        <span style={{ width: 90 }}>Modifier</span>
        <span>Action</span>
      </div>
      {row("None:", "verticalUnmodified", SIM_WHEEL_ACTIONS)}
      {/* `#ifdef __WXOSX_MAC__ m_lblVScrollCtrl->SetLabel( _( "Cmd" ) ); m_lblVScrollAlt->SetLabel( _( "Option" ) )` (no colon) */}
      {row(mac ? "Cmd" : "Ctrl:", "verticalWithCtrl", SIM_WHEEL_ACTIONS)}
      {row("Shift:", "verticalWithShift", SIM_WHEEL_ACTIONS)}
      {row(mac ? "Option" : "Alt:", "verticalWithAlt", SIM_WHEEL_ACTIONS)}
      <div style={{ fontSize: 12, opacity: 0.8, margin: "10px 0 4px" }}>Horizontal Touchpad Movement</div>
      <div className="filter-row" style={{ fontSize: 12, opacity: 0.75 }}>
        <span style={{ width: 90 }}>Modifier</span>
        <span>Action</span>
      </div>
      {row("Any:", "horizontal", SIM_WHEEL_HORIZONTAL)}
      <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
        <button data-testid="sim-wheel-mouse-defaults" onClick={() => setActions({ ...DEFAULT_SIM_WHEEL_ACTIONS })}>
          Reset to Mouse Defaults
        </button>
        <button data-testid="sim-wheel-trackpad-defaults" onClick={() => setActions({ ...TRACKPAD_SIM_WHEEL_ACTIONS })}>
          Reset to Trackpad Defaults
        </button>
      </div>
    </div>
  );
}
