// Preferences > PCB Editor > Origins & Axes (UI-LEFT-2 PP9): `PANEL_PCBNEW_DISPLAY_ORIGIN( .., FRAME_PCB_EDITOR )` (pcbnew/dialogs/panel_pcbnew_display_origin.cpp and
// its _base at KiCad 8303b2ad) -- "Display Origin" (Page origin, Drill/place file origin, Grid origin: `m_Display.m_DisplayOrigin`, shown for the board editor only,
// `m_displayOrigin->Show( m_frameType == FRAME_PCB_EDITOR )`), "X Axis" (Increases right / Increases left: `m_DisplayInvertXAxis`) and "Y Axis" (Increases up /
// Increases down: `m_DisplayInvertYAxis`). `TransferDataFromWindow` is the dialog's OK (the draft preferences); `ResetPanel` puts KiCad's defaults back (page origin,
// X to the right, Y down). What the settings do is kicad-port/originTransforms.ts.
import type { Preferences } from "../kicad-port/preferences";

/** The fields of `Preferences` the page owns (its `ResetPanel`). */
export const PCB_AXES_FIELDS: ReadonlyArray<keyof Preferences> = ["pcbDisplayOrigin", "pcbInvertXAxis", "pcbInvertYAxis"];

export function PcbOriginsAxesPage({ draft, set }: { draft: Preferences; set: (patch: Partial<Preferences>) => void }) {
  const heading = (text: string) => <div style={{ fontWeight: 600, margin: "10px 0 4px" }}>{text}</div>;
  const radio = (label: string, checked: boolean, onChange: () => void, testid: string) => (
    <label style={{ display: "block", margin: "2px 0 2px 12px" }}>
      <input type="radio" checked={checked} onChange={onChange} data-testid={testid} /> {label}
    </label>
  );
  return (
    <div data-testid="pcb-prefs-axes">
      {heading("Display Origin")}
      {radio("Page origin", draft.pcbDisplayOrigin === "page", () => set({ pcbDisplayOrigin: "page" }), "pcb-prefs-origin-page")}
      {radio("Drill/place file origin", draft.pcbDisplayOrigin === "aux", () => set({ pcbDisplayOrigin: "aux" }), "pcb-prefs-origin-aux")}
      {radio("Grid origin", draft.pcbDisplayOrigin === "grid", () => set({ pcbDisplayOrigin: "grid" }), "pcb-prefs-origin-grid")}
      {heading("X Axis")}
      {radio("Increases right", !draft.pcbInvertXAxis, () => set({ pcbInvertXAxis: false }), "pcb-prefs-x-right")}
      {radio("Increases left", draft.pcbInvertXAxis, () => set({ pcbInvertXAxis: true }), "pcb-prefs-x-left")}
      {heading("Y Axis")}
      {radio("Increases up", draft.pcbInvertYAxis, () => set({ pcbInvertYAxis: true }), "pcb-prefs-y-up")}
      {radio("Increases down", !draft.pcbInvertYAxis, () => set({ pcbInvertYAxis: false }), "pcb-prefs-y-down")}
    </div>
  );
}
