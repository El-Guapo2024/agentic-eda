// The editors' Editing Options pages of the Preferences dialog at KiCad 8303b2ad, every row of KiCad's page in its place: the rows the studio honours live,
// the others greyed with the reason as their tooltip (components/CommonPreferencesPages.tsx's `NotOffered` look).
//
//   * PCB Editor > Editing Options (`PANEL_EDIT_OPTIONS( .., false )`, pcbnew/dialogs/panel_edit_options.cpp and _base; `loadPCBSettings`, `TransferDataFromWindow`):
//     Constrain actions to H, V, 45 degrees (`m_AngleSnapMode`: checked is DEG45, unchecked DIRECT -- so OK, once the page was shown, makes a board in 90 degree mode
//     45, as KiCad's `TransferDataFromWindow` does), Step for rotate commands, Arc editing mode, Flip board items, the Magnetic Points, Show ratsnest with curved lines (the View
//     menu's toggle), Ratsnest line thickness, <ESC> clears net highlighting, Show courtyard collisions when moving/dragging. Greyed: Track mouse-drag mode,
//     Ctrl+click's Highlight net, Always show selected ratsnest, Show page limits. Automatically refill zones is live (UI-LEFT-2 PP10: kicad-cli fills,
//     kicad-port/autoRefill.ts).
//   * Schematic Editor > Editing Options (`PANEL_EESCHEMA_EDITING_OPTIONS`, eeschema/dialogs/panel_eeschema_editing_options.cpp and _base): Line drawing mode
//     (kept between sessions, `m_Drawing.line_mode`), Arc editing mode, Mouse drag performs Drag (G) operation (`m_Input.drag_is_move`), Automatically start wires on
//     unconnected pins (`m_Drawing.auto_start_wires`, `SCH_SELECTION_TOOL::autostartEvent`), <ESC> clears net highlighting, Allow unconstrained pin swaps, the new
//     sheets' border and background colours (`DrawSheet`), Power Symbols (`new_power_symbols`), Symbol Field Automatic Placement (`m_AutoplaceFields`: the server's
//     `AutoplacePrefs`), Repeated Items (Horizontal pitch, Vertical pitch, Label increment: what Repeat Last Item moves and counts by), Show footprint previews in
//     Symbol Chooser, Never show Rescue Symbols tool (`m_RescueNeverShow`). Greyed: Automatically annotate symbols (kept by design: no Annotate Automatically).
//   * Symbol Editor > Editing Options (`PANEL_SYM_EDITING_OPTIONS`, eeschema/dialogs/panel_sym_editing_options.cpp and _base): Defaults for New Objects (the line
//     width of a new shape or text box, the text size of a new text, text box or field, the first new pin's length and name and number sizes:
//     `SYMBOL_EDITOR_SETTINGS::m_Defaults`), Repeated Items (Pitch of repeated pins, forced to 25 mils on leaving the field, `onKillFocusPinPitch`; Label increment)
//     and Keep pins attached when dragging edges.
// The lengths are typed in mils (KiCad's `UNIT_BINDER` shows the frame's units). The Footprint Editor's page is components/FpPreferencesPages.tsx.
import type { ReactNode } from "react";
import { sanitizePinStepMils, sanitizeRatsnestThickness, type Preferences } from "../kicad-port/preferences";
import { ColorSwatch } from "./TextLookControls";
import type { ArcEditMode } from "../kicad-port/pcbPointEdit";
import type { LineMode } from "../kicad-port/schLineMode";
import { isMac } from "../platform";

/** What the pages edit besides the preferences: the board's line mode (`state.pcbx.angleSnapMode`), the ratsnest's curves (the View menu's toggle) and the
 *  schematic's line mode (`state.schLineMode`). */
export interface EditingExtra {
  pcbConstrain45: boolean;
  ratsnestCurved: boolean;
  schLineMode: LineMode;
}

/** `m_arcEditMode` / `m_choiceArcMode`'s choices, in `arcEditModeToComboIndex`'s order. */
const ARC_MODE_CHOICES: ReadonlyArray<[ArcEditMode, string]> = [
  ["keep_center_adjust_angle_radius", "Keep center, adjust radius"],
  ["keep_endpoints_or_start_direction", "Keep endpoints or direction of starting point"],
  ["keep_center_ends_adjust_angle", "Keep center and radius, adjust endpoints"],
];

const heading = (text: string) => <div style={{ fontWeight: 600, margin: "10px 0 4px" }}>{text}</div>;

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

const check = (label: string, checked: boolean, onChange: (v: boolean) => void, testid: string, tip?: string) => (
  <label className="filter-row" title={tip}>
    <input type="checkbox" data-testid={testid} checked={checked} onChange={(e) => onChange(e.target.checked)} />
    {label}
  </label>
);

function arcModeRow(value: ArcEditMode, onChange: (m: ArcEditMode) => void, testid: string) {
  return (
    <div className="filter-row">
      <span style={{ width: 170 }}>Arc editing mode:</span>
      <select aria-label="Arc editing mode:" data-testid={testid} value={value} onChange={(e) => onChange(e.target.value as ArcEditMode)}>
        {ARC_MODE_CHOICES.map(([m, label]) => (
          <option key={m} value={m}>
            {label}
          </option>
        ))}
      </select>
    </div>
  );
}

/** `m_mouseCmdsWinLin` / `m_mouseCmdsOSX`: what a left click does with the modifier keys (text only, but the Ctrl row of the board editor's). */
function LeftClickCommands({ ctrlRow }: { ctrlRow?: ReactNode }) {
  const mac = isMac();
  const rows: Array<[string, string]> = mac
    ? [["Click:", "Select item(s)"], ["Long Click:", "Clarify selection from menu"], ["Shift:", "Add item(s) to selection"], ["Shift+Cmd:", "Remove item(s) from selection"], ["Option:", "Clarify selection from menu"]]
    : [["Click:", "Select item(s)"], ["Long Click:", "Clarify selection from menu"], ["Shift:", "Add item(s) to selection"], ["Ctrl+Shift:", "Remove item(s) from selection"]];
  return (
    <div style={{ fontSize: 12 }}>
      <div style={{ fontStyle: "italic", opacity: 0.75, marginBottom: 4, whiteSpace: "pre-line" }}>
        {mac ? "Left click (and drag) actions depend on 3 modifier keys:\nOption, Shift and Cmd" : "Left click (and drag) actions depend on 2 modifier keys:\nShift and Ctrl"}
      </div>
      <div className="kv-grid" style={{ gridTemplateColumns: "90px 1fr", rowGap: 2 }}>
        {rows.map(([k, v]) => (
          <span key={k} style={{ display: "contents" }}>
            <span>{k}</span>
            <span>{v}</span>
          </span>
        ))}
      </div>
      {ctrlRow}
    </div>
  );
}

interface PageProps {
  draft: Preferences;
  set: (patch: Partial<Preferences>) => void;
  extra: EditingExtra;
  setExtra: (patch: Partial<EditingExtra>) => void;
  /** The board editor's rotation step as typed (`m_rotationAngle`, read on OK). */
  rotationText: string;
  setRotationText: (t: string) => void;
}

/** Preferences > PCB Editor > Editing Options. */
export function PcbEditingOptionsPage({ draft, set, extra, setExtra, rotationText, setRotationText }: PageProps) {
  const magnetic = (label: string, key: "magneticPads" | "magneticTracks", tip: string) => (
    <div className="filter-row" title={tip}>
      <span style={{ width: 170 }}>{label}</span>
      <select aria-label={label} value={draft[key]} onChange={(e) => set({ [key]: e.target.value as Preferences[typeof key] })}>
        <option value="never">Never</option>
        <option value="track-tool">When routing tracks</option>
        <option value="always">Always</option>
      </select>
    </div>
  );
  return (
    <div data-testid="pcb-editing-options" style={{ display: "flex", gap: 24, flexWrap: "wrap" }}>
      <div style={{ flex: "1 1 330px", minWidth: 0 }}>
        {heading("Editing Options")}
        {check("Constrain actions to H, V, 45 degrees", extra.pcbConstrain45, (v) => setExtra({ pcbConstrain45: v }), "pref-pcb-constrain45")}
        <div className="filter-row" title="Set increment (in degrees) for context menu and hotkey rotation.">
          <span style={{ width: 170 }}>Step for rotate commands:</span>
          <input
            type="number"
            aria-label="Step for rotate commands"
            data-testid="pref-rotation-step"
            style={{ width: 70 }}
            min={0.1}
            max={359.9}
            step={0.1}
            value={rotationText}
            onChange={(e) => setRotationText(e.target.value)}
          />
          <span>deg</span>
        </div>
        {arcModeRow(draft.pcbArcEditMode, (m) => set({ pcbArcEditMode: m }), "pref-pcb-arc-mode")}
        <NotOffered reason="Not ported: a mouse drag of a track starts the router's drag (KiCad's default, Drag (45 degree mode)).">
          <span style={{ width: 170 }}>Track mouse-drag mode:</span>
          <select disabled value="1" aria-label="Track mouse-drag mode:">
            <option value="0">Move</option>
            <option value="1">Drag (45 degree mode)</option>
            <option value="2">Drag (free angle)</option>
          </select>
        </NotOffered>
        <div className="filter-row" title="Which way Change Side / Flip turns a board item over.">
          <span style={{ width: 170 }}>Flip board items:</span>
          <label style={{ marginRight: 12 }}>
            <input type="radio" name="flip-direction" data-testid="pref-flip-left-right" checked={draft.flipDirection === "left_right"} onChange={() => set({ flipDirection: "left_right" })} /> Left/right
          </label>
          <label>
            <input type="radio" name="flip-direction" data-testid="pref-flip-top-bottom" checked={draft.flipDirection === "top_bottom"} onChange={() => set({ flipDirection: "top_bottom" })} /> Top/bottom
          </label>
        </div>
        {check("Allow free pads", draft.allowFreePads, (v) => set({ allowFreePads: v }), "pref-allow-free-pads", "If checked, pads can be moved with respect to the rest of the footprint.")}
        {heading("Left Click Mouse Commands")}
        <LeftClickCommands
          ctrlRow={
            <NotOffered reason="Not ported: Ctrl+click toggles the selection (KiCad's default); Highlight Net is its own tool here.">
              <span style={{ width: 86 }}>{isMac() ? "Cmd:" : "Ctrl:"}</span>
              <label style={{ marginRight: 12 }}>
                <input type="radio" disabled checked readOnly /> Toggle selection
              </label>
              <label>
                <input type="radio" disabled checked={false} readOnly /> Highlight net (for pads/tracks)
              </label>
            </NotOffered>
          }
        />
      </div>
      <div style={{ flex: "1 1 300px", minWidth: 0 }}>
        {heading("Magnetic Points")}
        {magnetic("Snap to pads:", "magneticPads", "Capture cursor when the mouse enters a pad area")}
        {magnetic("Snap to tracks and vias:", "magneticTracks", "Capture cursor when the mouse approaches a track")}
        <div className="filter-row" title="Capture cursor when the mouse approaches graphical control points">
          <span style={{ width: 170 }}>Snap to graphics:</span>
          <select aria-label="Snap to graphics:" value={draft.magneticGraphics ? "always" : "never"} onChange={(e) => set({ magneticGraphics: e.target.value === "always" })}>
            <option value="always">Always</option>
            <option value="never">Never</option>
          </select>
        </div>
        {heading("Ratsnest")}
        {offCheck("Always show selected ratsnest", true, "Not ported: a selected footprint's ratsnest is shown with the board's (the Ratsnest layer's visibility).")}
        {check("Show ratsnest with curved lines", extra.ratsnestCurved, (v) => setExtra({ ratsnestCurved: v }), "pref-ratsnest-curved")}
        <div className="filter-row">
          <span style={{ width: 170 }}>Ratsnest line thickness:</span>
          <input
            type="number"
            aria-label="Ratsnest line thickness:"
            data-testid="pref-ratsnest-thickness"
            style={{ width: 70 }}
            min={0.5}
            max={10}
            step={0.5}
            value={draft.ratsnestThicknessPx}
            onChange={(e) => set({ ratsnestThicknessPx: sanitizeRatsnestThickness(Number(e.target.value), draft.ratsnestThicknessPx) })}
          />
        </div>
        {heading("Miscellaneous")}
        {check("<ESC> clears net highlighting", draft.escClearsNetHighlight, (v) => set({ escClearsNetHighlight: v }), "pref-pcb-esc-clears-highlight")}
        {offCheck("Show page limits", true, "Not ported: the board editor does not draw the page limits. KiCad: Draw an outline to show the sheet size.")}
        {check("Show courtyard collisions when moving/dragging", draft.showCourtyardCollisions, (v) => set({ showCourtyardCollisions: v }), "pref-courtyard-collisions")}
        {check("Automatically refill zones", draft.autoRefillZones, (v) => set({ autoRefillZones: v }), "pref-auto-refill-zones", "If checked, zones will be re-filled after each edit operation")}
      </div>
    </div>
  );
}

const milsInput = (label: string, value: number, onChange: (v: number) => void, testid: string, onBlur?: () => void) => (
  <div className="filter-row">
    <span style={{ width: 170 }}>{label}</span>
    <input type="number" aria-label={label} data-testid={testid} style={{ width: 70 }} value={value} onChange={(e) => onChange(Number(e.target.value))} onBlur={onBlur} />
    <span>mils</span>
  </div>
);

/** Preferences > Schematic Editor > Editing Options. */
export function SchEditingOptionsPage({ draft, set, extra, setExtra }: PageProps) {
  return (
    <div data-testid="sch-editing-options" style={{ display: "flex", gap: 24, flexWrap: "wrap" }}>
      <div style={{ flex: "1 1 330px", minWidth: 0 }}>
        {heading("Editing")}
        <div className="filter-row">
          <span style={{ width: 170 }}>Line drawing mode:</span>
          <select aria-label="Line drawing mode:" data-testid="pref-sch-line-mode" value={String(extra.schLineMode)} onChange={(e) => setExtra({ schLineMode: Number(e.target.value) as LineMode })}>
            <option value="0">Free Angle</option>
            <option value="1">90 deg Angle</option>
            <option value="2">45 deg Angle</option>
          </select>
        </div>
        {arcModeRow(draft.schArcEditMode, (m) => set({ schArcEditMode: m }), "pref-sch-arc-mode")}
        {check("Mouse drag performs Drag (G) operation", !draft.schDragIsMove, (v) => set({ schDragIsMove: !v }), "pref-sch-mouse-drag-is-drag", "If unchecked, mouse drag will perform move (M) operation")}
        {check("Automatically start wires on unconnected pins", draft.schAutoStartWires, (v) => set({ schAutoStartWires: v }), "pref-sch-auto-start-wires", "When enabled, you can start wiring by clicking on unconnected pins even when the wire tool is not active")}
        {check("<ESC> clears net highlighting", draft.schEscClearsNetHighlight, (v) => set({ schEscClearsNetHighlight: v }), "pref-sch-esc-clears-highlight", "First <ESC> in selection tool clears selection, next clears net highlighting")}
        {offCheck("Automatically annotate symbols", true, "Kept by design: placed symbols are always annotated (KiCad's default); the studio has no Annotate Automatically switch.")}
        {check("Allow unconstrained pin swaps", draft.allowUnconstrainedPinSwaps, (v) => set({ allowUnconstrainedPinSwaps: v }), "pref-allow-pin-swaps", "Allows swapping symbol pins' positions. May cause invalid design changes; use with caution.")}
        {heading("Defaults for New Objects")}
        {/* `m_borderColorSwatch` / `m_backgroundColorSwatch` ("Clear Color" is the default: `COLOR4D::UNSPECIFIED`, the layer's colour) */}
        <div className="filter-row" data-testid="pref-sch-sheet-border">
          <span style={{ width: 120 }}>Sheet border:</span>
          <ColorSwatch label="Sheet border:" color={draft.schSheetBorderColor} onChange={(c) => set({ schSheetBorderColor: c })} />
        </div>
        <div className="filter-row" data-testid="pref-sch-sheet-background">
          <span style={{ width: 120 }}>Sheet background:</span>
          <ColorSwatch label="Sheet background:" color={draft.schSheetBackgroundColor} onChange={(c) => set({ schSheetBackgroundColor: c })} />
        </div>
        <div className="filter-row">
          <span style={{ width: 120 }}>Power Symbols:</span>
          <select
            aria-label="Power Symbols:"
            data-testid="pref-sch-power-symbols"
            value={String(draft.schNewPowerSymbols)}
            onChange={(e) => set({ schNewPowerSymbols: Number(e.target.value) as 0 | 1 | 2 })}
            title={"Select the assigned type of new power symbol.\nDefault will follow the symbol definition.\nGlobal will convert new power symbols to global power symbols.\nLocal will convert new power symbols to a local power symbols."}
          >
            <option value="0">Default</option>
            <option value="1">Global</option>
            <option value="2">Local</option>
          </select>
        </div>
        {heading("Left Click Mouse Commands")}
        <LeftClickCommands />
      </div>
      <div style={{ flex: "1 1 300px", minWidth: 0 }}>
        {heading("Symbol Field Automatic Placement")}
        {check("Automatically place symbol fields", draft.schAutoplaceFields, (v) => set({ schAutoplaceFields: v }), "pref-sch-autoplace-fields")}
        {check("Allow field autoplace to change justification", draft.schAutoplaceJustify, (v) => set({ schAutoplaceJustify: v }), "pref-sch-autoplace-justify")}
        {check("Always align autoplaced fields to the 50 mil grid", draft.schAutoplaceAlign, (v) => set({ schAutoplaceAlign: v }), "pref-sch-autoplace-align")}
        {heading("Repeated Items")}
        {milsInput("Horizontal pitch:", draft.schRepeatOffsetXMils, (v) => set({ schRepeatOffsetXMils: Number.isFinite(v) ? Math.round(v) : 0 }), "pref-sch-repeat-x")}
        {milsInput("Vertical pitch:", draft.schRepeatOffsetYMils, (v) => set({ schRepeatOffsetYMils: Number.isFinite(v) ? Math.round(v) : 0 }), "pref-sch-repeat-y")}
        <div className="filter-row">
          <span style={{ width: 170 }}>Label increment:</span>
          <input type="number" aria-label="Label increment (schematic)" data-testid="pref-sch-label-increment" style={{ width: 70 }} min={-100000} max={100000} step={1} value={draft.schRepeatLabelIncrement} onChange={(e) => set({ schRepeatLabelIncrement: Math.round(Number(e.target.value)) || 0 })} />
        </div>
        {heading("Dialog Preferences")}
        {check("Show footprint previews in Symbol Chooser", draft.schFootprintPreview, (v) => set({ schFootprintPreview: v }), "pref-sch-footprint-preview")}
        {check("Never show Rescue Symbols tool", draft.schRescueNeverShow, (v) => set({ schRescueNeverShow: v }), "pref-sch-never-show-rescue")}
      </div>
    </div>
  );
}

/** Preferences > Symbol Editor > Editing Options. `fallbackPinStep`: the step kept when the field holds nothing usable. */
export function SymEditingOptionsPage({ draft, set, fallbackPinStep }: Pick<PageProps, "draft" | "set"> & { fallbackPinStep: number }) {
  // `m_lineWidth`, `m_textSize`, `m_pinLength`, `m_pinNumberSize`, `m_pinNameSize` (`UNIT_BINDER`s; `TransferDataFromWindow` keeps whole mils)
  const defaults: Array<[string, keyof Preferences, string]> = [
    ["Default line width:", "symDefaultLineWidthMils", "pref-sym-default-line-width"],
    ["Default text size:", "symDefaultTextSizeMils", "pref-sym-default-text-size"],
    ["Default pin length:", "symDefaultPinLengthMils", "pref-sym-default-pin-length"],
    ["Default pin number size:", "symDefaultPinNumSizeMils", "pref-sym-default-pin-num-size"],
    ["Default pin name size:", "symDefaultPinNameSizeMils", "pref-sym-default-pin-name-size"],
  ];
  return (
    <div data-testid="sym-editing-options">
      {heading("Defaults for New Objects")}
      {defaults.map(([label, key, testid]) => (
        <div key={label}>{milsInput(label, draft[key] as number, (v) => set({ [key]: Number.isFinite(v) ? Math.max(0, Math.round(v)) : 0 } as Partial<Preferences>), testid)}</div>
      ))}
      <div style={{ fontStyle: "italic", fontSize: 11, opacity: 0.7, marginLeft: 4 }}>Set to 0 to allow symbols to inherit line width properties{"\n"}from schematic</div>
      {heading("Repeated Items")}
      {milsInput("Pitch of repeated pins:", draft.symRepeatPinStepMils, (v) => set({ symRepeatPinStepMils: v }), "pref-sym-pin-pitch", () => set({ symRepeatPinStepMils: sanitizePinStepMils(draft.symRepeatPinStepMils, fallbackPinStep) }))}
      <div className="filter-row" title="What Repeat Pin adds to the pin's number and name.">
        <span style={{ width: 170 }}>Label increment:</span>
        <input type="number" aria-label="Label increment" data-testid="pref-sym-label-increment" style={{ width: 70 }} min={-10} max={10} step={1} value={draft.symRepeatLabelDelta} onChange={(e) => set({ symRepeatLabelDelta: Number(e.target.value) })} />
      </div>
      {heading("General Editing")}
      {check("Keep pins attached when dragging edges", draft.symDragPinsWithEdges, (v) => set({ symDragPinsWithEdges: v }), "pref-sym-drag-pins-with-edges")}
    </div>
  );
}
