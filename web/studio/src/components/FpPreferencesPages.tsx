// Preferences > Footprint Editor (UI-LEFT F8): the pages KiCad's Preferences dialog has for the Footprint Editor (`EDA_BASE_FRAME::ShowPreferences`: "Footprint Editor" with
// Display Options, Grids, Origins & Axes, Editing Options, Colors, Footprint Defaults, Graphics Defaults ... among its sub-pages), each one ported from its panel at KiCad 8303b2ad:
//   - Display Options: `PANEL_DISPLAY_OPTIONS` of the frame -- the cursor and the grid display, every editor's own page (components/PreferencesDialog.tsx's `display_footprint`);
//   - Origins & Axes: `PANEL_PCBNEW_DISPLAY_ORIGIN( .., FRAME_FOOTPRINT_EDITOR )` -- no Display Origin (`m_displayOrigin->Show( m_frameType == FRAME_PCB_EDITOR )`), the
//     X axis increasing right or left and the Y axis down or up (`m_DisplayInvertXAxis`, `m_DisplayInvertYAxis`), which the status line's coordinates follow;
//   - Editing Options: `PANEL_EDIT_OPTIONS( .., true )` -- "Constrain actions to H, V, 45 degrees" (`m_AngleSnapMode`, the editor's line mode), "Step for rotate commands:"
//     (`m_RotationAngle`, which R and Shift+R turn by), "Arc editing mode:", the Magnetic Points' Snap to pads and Snap to graphics (`loadFPSettings` / `TransferDataFromWindow`);
//   - Graphics Defaults: `PANEL_FP_EDITOR_GRAPHICS_DEFAULTS`' grid of the layer classes (line thickness, text width, height and thickness, italic, keep upright), which the
//     drawing tools and the Text tool start from (kicad-port/fpDrawDefaults.ts, fpTextTool.ts); "Reset" puts KiCad's factory table back.
// Not ported: the Grids, Colors, Toolbars, Footprint Defaults and User Layer Names pages; the arc editing mode is kept, not yet read by the point editor.
import { TEXT_GRAPHICS_ROWS, textRowOf, validateTextGraphics, withLineWidth, withTextFields } from "../kicad-port/boardSetupRules";
import { FACTORY_TEXT_GRAPHICS } from "../kicad-port/layerDefaults";
import type { Preferences } from "../kicad-port/preferences";
import type { LayerClassDefaults, TextGraphicsDefaults } from "../api/types";
import type { AngleSnapMode } from "../kicad-port/pcbParityState";
import { nmFrom, nmTo, type LengthUnit } from "../state/units";

export type FpPrefsPage = "fp_axes" | "fp_edit" | "fp_graphics";

/** The tree's rows of the Footprint Editor after its Display Options (`book->AddPage( new wxPanel( book ), _( "Footprint Editor" ) )`'s sub-pages), in KiCad's order. */
export const FP_PREFS_SUBTREE: Array<[FpPrefsPage, string, number]> = [
  ["fp_axes", "Origins & Axes", 1],
  ["fp_edit", "Editing Options", 1],
  ["fp_graphics", "Graphics Defaults", 1],
];

/** The page titles the dialog's header shows. */
export const FP_PREFS_TITLES: Record<FpPrefsPage, string> = { fp_axes: "Origins & Axes", fp_edit: "Editing Options", fp_graphics: "Graphics Defaults" };

/** `m_arcEditModeChoices`. */
const ARC_EDIT_MODES = ["Keep center, adjust radius", "Keep endpoints or direction of starting point", "Keep center and radius, adjust endpoints"];

/** What the pages edit besides the preferences: the editor's line mode. */
export interface FpPrefsExtra {
  lineMode: AngleSnapMode;
}

/** `PANEL_FP_EDITOR_GRAPHICS_DEFAULTS::TransferDataFromWindow`'s checks (the Board Setup grid's: kicad-port/boardSetupRules.ts); the first message, or null. */
export function fpGraphicsProblem(t: TextGraphicsDefaults | null): string | null {
  if (!t) return null;
  const errors = validateTextGraphics(t);
  const first = TEXT_GRAPHICS_ROWS.find((r) => errors[r.id]);
  return first ? `${first.label}: ${errors[first.id]}` : null;
}

export function FpPreferencesPage(props: {
  page: FpPrefsPage;
  draft: Preferences;
  set: (patch: Partial<Preferences>) => void;
  extra: FpPrefsExtra;
  setExtra: (patch: Partial<FpPrefsExtra>) => void;
  rotationText: string;
  setRotationText: (t: string) => void;
  units: LengthUnit;
}) {
  const { page, draft, set, extra, setExtra, units } = props;
  const heading = (text: string) => <div style={{ fontWeight: 600, margin: "10px 0 4px" }}>{text}</div>;
  const radio = (label: string, checked: boolean, onChange: () => void, testid: string) => (
    <label style={{ display: "block", margin: "2px 0" }}>
      <input type="radio" checked={checked} onChange={onChange} data-testid={testid} /> {label}
    </label>
  );
  const check = (label: string, checked: boolean, onChange: (v: boolean) => void, testid: string, title?: string) => (
    <label style={{ display: "block", margin: "2px 0" }} title={title}>
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} data-testid={testid} /> {label}
    </label>
  );

  if (page === "fp_axes") {
    return (
      <div data-testid="fp-prefs-axes">
        {heading("X Axis")}
        {radio("Increases right", !draft.fpInvertXAxis, () => set({ fpInvertXAxis: false }), "fp-prefs-x-right")}
        {radio("Increases left", draft.fpInvertXAxis, () => set({ fpInvertXAxis: true }), "fp-prefs-x-left")}
        {heading("Y Axis")}
        {radio("Increases up", draft.fpInvertYAxis, () => set({ fpInvertYAxis: true }), "fp-prefs-y-up")}
        {radio("Increases down", !draft.fpInvertYAxis, () => set({ fpInvertYAxis: false }), "fp-prefs-y-down")}
      </div>
    );
  }
  if (page === "fp_edit") {
    return (
      <div data-testid="fp-prefs-edit">
        {heading("Editing Options")}
        {check("Constrain actions to H, V, 45 degrees", extra.lineMode !== "direct", (v) => setExtra({ lineMode: v ? "45" : "direct" }), "fp-prefs-constrain45")}
        <label style={{ display: "flex", gap: 8, alignItems: "center", margin: "4px 0" }} title="Set increment (in degrees) for context menu and hotkey rotation.">
          Step for rotate commands:
          <input value={props.rotationText} onChange={(e) => props.setRotationText(e.target.value)} style={{ width: 70 }} data-testid="fp-prefs-rotation" /> deg
        </label>
        <label style={{ display: "flex", gap: 8, alignItems: "center", margin: "4px 0" }}>
          Arc editing mode:
          <select value={draft.fpArcEditMode} onChange={(e) => set({ fpArcEditMode: Number(e.target.value) })}>
            {ARC_EDIT_MODES.map((m, i) => (
              <option key={m} value={i}>
                {m}
              </option>
            ))}
          </select>
        </label>
        {heading("Magnetic Points")}
        {check("Snap to pads", draft.fpMagneticPads, (v) => set({ fpMagneticPads: v }), "fp-prefs-magnetic-pads")}
        {check("Snap to graphics", draft.fpMagneticGraphics, (v) => set({ fpMagneticGraphics: v }), "fp-prefs-magnetic-graphics")}
      </div>
    );
  }
  // Graphics Defaults
  const t = draft.fpTextGraphics ?? FACTORY_TEXT_GRAPHICS;
  const edit = (f: (d: TextGraphicsDefaults) => TextGraphicsDefaults) => set({ fpTextGraphics: f(t) });
  const problem = fpGraphicsProblem(draft.fpTextGraphics);
  const cell = (value: number, onValue: (v: number) => void, label: string) => (
    <td>
      <input
        aria-label={label}
        style={{ width: 70 }}
        defaultValue={String(Number(nmTo(value, units).toFixed(4)))}
        onChange={(e) => {
          const v = Number(e.target.value);
          onValue(Number.isFinite(v) ? Math.round(nmFrom(v, units)) : Number.NaN);
        }}
      />
    </td>
  );
  return (
    <div data-testid="fp-prefs-graphics">
      {heading("Default Properties for New Graphic Items")}
      <table style={{ borderCollapse: "collapse", fontSize: 12 }}>
        <thead>
          <tr>
            <th />
            <th>Line Thickness</th>
            <th>Text Width</th>
            <th>Text Height</th>
            <th>Text Thickness</th>
            <th>Italic</th>
            <th>Keep Upright</th>
          </tr>
        </thead>
        <tbody>
          {TEXT_GRAPHICS_ROWS.map((row) => {
            const values = textRowOf(t, row.id);
            const text = row.hasText ? (values as LayerClassDefaults) : null;
            const textId = row.id as "silk" | "copper" | "fab" | "others";
            const setText = (patch: Partial<LayerClassDefaults>) => edit((d) => withTextFields(d, textId, patch));
            return (
              <tr key={row.id}>
                <th scope="row" style={{ textAlign: "left", paddingRight: 8 }}>
                  {row.label}
                </th>
                {cell(values.line_width_nm, (v) => edit((d) => withLineWidth(d, row.id, v)), `${row.label} Line Thickness`)}
                {text ? cell(text.text_width_nm, (v) => setText({ text_width_nm: v }), `${row.label} Text Width`) : <td />}
                {text ? cell(text.text_height_nm, (v) => setText({ text_height_nm: v }), `${row.label} Text Height`) : <td />}
                {text ? cell(text.text_thickness_nm, (v) => setText({ text_thickness_nm: v }), `${row.label} Text Thickness`) : <td />}
                <td>{text && <input type="checkbox" aria-label={`${row.label} Italic`} checked={text.italic} onChange={(e) => setText({ italic: e.target.checked })} />}</td>
                <td>{text && <input type="checkbox" aria-label={`${row.label} Keep Upright`} checked={text.upright} onChange={(e) => setText({ upright: e.target.checked })} />}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <p style={{ fontSize: 11, opacity: 0.7 }}>Sizes are in {units}.</p>
      {problem && (
        <div role="alert" style={{ color: "#ff6666", fontSize: 12 }}>
          {problem}
        </div>
      )}
    </div>
  );
}
