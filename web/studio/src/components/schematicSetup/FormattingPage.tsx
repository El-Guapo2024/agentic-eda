// Schematic Setup > General > Formatting -- port of eeschema/dialogs/panel_setup_formatting.cpp (`TransferDataToWindow` / `TransferDataFromWindow`), every row of
// the panel in its boxes and order, with its units and KiCad's defaults (`SCHEMATIC_SETTINGS::SCHEMATIC_SETTINGS`):
//
//   Text                     Default text size (mils), Overbar offset ratio (%), Label offset ratio (%), Global label margin ratio (%)
//   Symbols                  Default line width (mils), Pin symbol size (mils)
//   Connections              Junction dot size (None .. Largest), Hop-over size (None .. Largest), Connection grid (mils)
//   Inter-sheet References   Show inter-sheet references, Show own page reference, Standard (1,2,3) / Abbreviated (1..3), Prefix, Suffix
//                            (`onCheckBoxIref` greys the others out while they are not shown)
//   Dashed Lines             Dash length, Gap length ("Dash and dot lengths are ratios of the line width.")
//   Operating-point Overlay  Significant digits (voltages) 1..10, Range (voltages), Significant digits (currents), Range (currents)
//
// The fields read their text as KiCad's do (`DoubleValueFromString`: the leading number, 0 for none); the panel's one check is the connection grid's
// (`m_connectionGridSize.Validate( MIN_CONNECTION_GRID_MILS, 10000, MILS )`), refused with its words before anything is applied. The sizes are kept as typed and
// read back as KiCad reads them (`SchFormatting::effective`: a size out of its `PARAM_SCALED` range is the default). The connection grid is ERC's
// `endpoint_off_grid` grid (a marker menu's "Edit connection grid spacing..." opens this page). Apply is one undo step: `set_connection_grid`,
// `set_intersheet_refs` and `set_sch_formatting`, each only when its part changed, in one `batch`. The sheet draws the junction dots at their size
// (schematic/painter.ts); kicad-cli reads the rest from the derived project's `schematic.drawing`.
import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchSchFormatting } from "../../api/client";
import type { Cmd, IntersheetRefs, SchFormatting, SchFormattingReply } from "../../api/types";
import { DEFAULT_SCH_FORMATTING, HOP_OVER_SIZE_CHOICES, I_RANGES, JUNCTION_SIZE_CHOICES, V_RANGES, connectionGridError, doubleValueFromString, milsText, milsToNm, rangeLabel, rangeStored, ratioText, samePageSettings } from "../../kicad-port/schSetup";
import { useStudioState } from "../../state/store";
import { Check, FieldRow, PageFrame, usePageApply, usePageDraft, type PageProps } from "../boardSetup/fields";

/** `SCHEMATIC_SETTINGS`' defaults (`DEFAULT_IREF_PREFIX`, `DEFAULT_IREF_SUFFIX`): what an older backend that sends none means. */
const IREF_DEFAULT: IntersheetRefs = { show: false, own_page: true, short: false, prefix: "[", suffix: "]" };

/** The page's text fields, as typed. */
interface Texts {
  grid: string;
  textSize: string;
  overbar: string;
  textOffset: string;
  labelSize: string;
  lineWidth: string;
  pinSymbol: string;
  dash: string;
  gap: string;
}

interface Draft {
  texts: Texts;
  junction: number;
  hop: number;
  vPrecision: number;
  vRange: string;
  iPrecision: number;
  iRange: string;
  iref: IntersheetRefs;
}

/** `TransferDataToWindow`: the sizes in mils, the ratios as percentages (`* 100.0`), the dash and gap as they are. */
function draftOf(r: SchFormattingReply): Draft {
  const f = r.formatting ?? DEFAULT_SCH_FORMATTING;
  return {
    texts: {
      grid: String(r.connection_grid_mils),
      textSize: milsText(f.default_text_size_nm),
      overbar: ratioText(f.overbar_offset_ratio * 100),
      textOffset: ratioText(f.text_offset_ratio * 100),
      labelSize: ratioText(f.label_size_ratio * 100),
      lineWidth: milsText(f.default_line_width_nm),
      pinSymbol: milsText(f.pin_symbol_size_nm),
      dash: ratioText(f.dash_ratio),
      gap: ratioText(f.gap_ratio),
    },
    junction: f.junction_size_choice,
    hop: f.hop_over_size_choice,
    vPrecision: f.opo_v_precision,
    vRange: rangeLabel(f.opo_v_range),
    iPrecision: f.opo_i_precision,
    iRange: rangeLabel(f.opo_i_range),
    iref: r.intersheet_refs ?? IREF_DEFAULT,
  };
}

/** `TransferDataFromWindow`: the settings the draft says (`GET_VALUE( PERCENT, .. ) / 100.0`, `GetIntValue` of the mils). */
function formattingOf(d: Draft): SchFormatting {
  const t = d.texts;
  return {
    default_text_size_nm: milsToNm(t.textSize),
    overbar_offset_ratio: doubleValueFromString(t.overbar) / 100,
    text_offset_ratio: doubleValueFromString(t.textOffset) / 100,
    label_size_ratio: doubleValueFromString(t.labelSize) / 100,
    default_line_width_nm: milsToNm(t.lineWidth),
    pin_symbol_size_nm: milsToNm(t.pinSymbol),
    junction_size_choice: d.junction,
    hop_over_size_choice: d.hop,
    dash_ratio: doubleValueFromString(t.dash),
    gap_ratio: doubleValueFromString(t.gap),
    opo_v_precision: d.vPrecision,
    opo_v_range: rangeStored(d.vRange, "V"),
    opo_i_precision: d.iPrecision,
    opo_i_range: rangeStored(d.iRange, "A"),
  };
}

export function FormattingPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const [info, setInfo] = useState<SchFormattingReply | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  // Read again when the design changes, so an Undo from outside the dialog shows up here.
  const load = useCallback(async () => {
    try {
      const r = await fetchSchFormatting();
      if (r.ok) {
        setInfo(r);
        setProblem(null);
      } else setProblem(r.message ?? "The backend could not read the schematic's formatting.");
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load, state.version]);

  const source = useMemo(() => (info ? draftOf(info) : undefined), [info]);
  const page = usePageDraft<Draft>(source);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);

  const d = page.draft;
  // `m_connectionGridSize.GetIntValue()`: the field's mils, rounded
  const grid = d ? Math.round(doubleValueFromString(d.texts.grid)) : undefined;
  const gridError = grid === undefined ? null : connectionGridError(grid, info?.min_connection_grid_mils ?? 25, info?.max_connection_grid_mils ?? 10_000);
  const setText = (k: keyof Texts) => (v: string) => page.edit((x) => ({ ...x, texts: { ...x.texts, [k]: v } }));
  const setIref = (change: Partial<IntersheetRefs>) => page.edit((x) => ({ ...x, iref: { ...x.iref, ...change } }));

  const onApply = () => {
    if (!d || !info || grid === undefined) return;
    if (gridError !== null) {
      apply.setMessage({ text: gridError, bad: true });
      return;
    }
    const cmds: Cmd[] = [];
    if (grid !== info.connection_grid_mils) cmds.push({ op: "set_connection_grid", mils: grid });
    if (!samePageSettings(d.iref, info.intersheet_refs ?? IREF_DEFAULT)) cmds.push({ op: "set_intersheet_refs", settings: d.iref });
    const f = formattingOf(d);
    if (!samePageSettings(f, info.formatting ?? DEFAULT_SCH_FORMATTING)) cmds.push({ op: "set_sch_formatting", settings: f });
    if (cmds.length === 0) {
      page.markClean();
      return;
    }
    void apply.run(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds }, () => {
      page.markClean();
      void load();
    });
  };

  const text = (k: keyof Texts, aria: string, title?: string) => <input className="bs-input" aria-label={aria} title={title} value={d?.texts[k] ?? ""} onChange={(e) => setText(k)(e.target.value)} style={{ width: 80 }} spellCheck={false} />;
  const choice = (value: number, list: readonly string[], aria: string, set: (v: number) => void) => (
    <select aria-label={aria} value={value} onChange={(e) => set(Number(e.target.value))}>
      {list.map((c, i) => (
        <option key={c} value={i}>
          {c}
        </option>
      ))}
    </select>
  );
  const range = (value: string, list: readonly string[], aria: string, set: (v: string) => void) => (
    <select aria-label={aria} value={value} onChange={(e) => set(e.target.value)}>
      {list.map((c) => (
        <option key={c} value={c}>
          {c}
        </option>
      ))}
    </select>
  );
  const spin = (value: number, aria: string, set: (v: number) => void) => (
    // `wxSpinCtrl( .., 1, 10, 3 )`
    <input type="number" className="bs-input" aria-label={aria} min={1} max={10} value={value} onChange={(e) => set(Math.min(10, Math.max(1, Math.round(Number(e.target.value) || 1))))} style={{ width: 60 }} />
  );

  return (
    <PageFrame title="Formatting" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? problem ?? (page.dirty ? gridError : null)} bad={apply.message?.bad || problem !== null || (page.dirty && gridError !== null)} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!d ? (
        <p className="bs-note">{problem ?? "Reading the schematic's formatting…"}</p>
      ) : (
        <div style={{ display: "flex", gap: 24, flexWrap: "wrap" }}>
          <div>
            <h4 className="bs-subhead">Text</h4>
            <FieldRow label="Default text size:" unit="mils" title="SCHEMATIC_SETTINGS::m_DefaultTextSize">
              {text("textSize", "Default text size in mils")}
            </FieldRow>
            <FieldRow label="Overbar offset ratio:" unit="%" title="FONT_METRICS::m_OverbarHeight">
              {text("overbar", "Overbar offset ratio in percent")}
            </FieldRow>
            <FieldRow label="Label offset ratio:" unit="%" title="Percentage of the text size to offset labels above (or below) a wire, bus, or pin">
              {text("textOffset", "Label offset ratio in percent", "Percentage of the text size to offset labels above (or below) a wire, bus, or pin")}
            </FieldRow>
            <FieldRow label="Global label margin ratio:" unit="%" title="Percentage of the text size to use as space around a global label">
              {text("labelSize", "Global label margin ratio in percent", "Percentage of the text size to use as space around a global label")}
            </FieldRow>
            <h4 className="bs-subhead">Symbols</h4>
            <FieldRow label="Default line width:" unit="mils" title="SCHEMATIC_SETTINGS::m_DefaultLineWidth">
              {text("lineWidth", "Default line width in mils")}
            </FieldRow>
            <FieldRow label="Pin symbol size:" unit="mils" title="SCHEMATIC_SETTINGS::m_PinSymbolSize">
              {text("pinSymbol", "Pin symbol size in mils")}
            </FieldRow>
            <h4 className="bs-subhead">Connections</h4>
            <FieldRow label="Junction dot size:" title="SCHEMATIC_SETTINGS::m_JunctionSizeChoice: the Default net class's wire width times 0, 1.7, 4, 6, 9 or 12">
              {choice(d.junction, JUNCTION_SIZE_CHOICES, "Junction dot size", (v) => page.edit((x) => ({ ...x, junction: v })))}
            </FieldRow>
            <FieldRow label="Hop-over size:" title="SCHEMATIC_SETTINGS::m_HopOverSizeChoice">
              {choice(d.hop, HOP_OVER_SIZE_CHOICES, "Hop-over size", (v) => page.edit((x) => ({ ...x, hop: v })))}
            </FieldRow>
            <FieldRow label="Connection grid:" unit="mils" error={page.dirty ? gridError : null} title="SCHEMATIC_SETTINGS::m_ConnectionGridSize: the grid ERC expects wire ends and symbol pins on (Symbol pin or wire end off connection grid)">
              <input className={`bs-input${gridError ? " bs-invalid" : ""}`} aria-label="Connection grid spacing in mils" value={d.texts.grid} onChange={(e) => setText("grid")(e.target.value)} style={{ width: 80 }} spellCheck={false} />
            </FieldRow>
          </div>
          <div>
            <h4 className="bs-subhead">Inter-sheet References</h4>
            <Check checked={d.iref.show} onChange={(v) => setIref({ show: v })} title="SCHEMATIC_SETTINGS::m_IntersheetRefsShow">
              Show inter-sheet references
            </Check>
            <Check checked={d.iref.own_page} disabled={!d.iref.show} onChange={(v) => setIref({ own_page: v })} title="m_IntersheetRefsListOwnPage">
              Show own page reference
            </Check>
            <label className="bs-check">
              <input type="radio" name="iref-format" aria-label="Standard (1,2,3)" checked={!d.iref.short} disabled={!d.iref.show} onChange={() => setIref({ short: false })} /> Standard (1,2,3)
            </label>
            <label className="bs-check">
              <input type="radio" name="iref-format" aria-label="Abbreviated (1..3)" checked={d.iref.short} disabled={!d.iref.show} onChange={() => setIref({ short: true })} /> Abbreviated (1..3)
            </label>
            <FieldRow label="Prefix:">
              <input aria-label="Inter-sheet references prefix" value={d.iref.prefix} disabled={!d.iref.show} onChange={(e) => setIref({ prefix: e.target.value })} style={{ width: 160 }} />
            </FieldRow>
            <FieldRow label="Suffix:">
              <input aria-label="Inter-sheet references suffix" value={d.iref.suffix} disabled={!d.iref.show} onChange={(e) => setIref({ suffix: e.target.value })} style={{ width: 160 }} />
            </FieldRow>
            <h4 className="bs-subhead">Dashed Lines</h4>
            <FieldRow label="Dash length:">{text("dash", "Dash length")}</FieldRow>
            <FieldRow label="Gap length:">{text("gap", "Gap length")}</FieldRow>
            <p className="bs-note" style={{ fontStyle: "italic" }}>
              Dash and dot lengths are ratios of the line width.
            </p>
            <h4 className="bs-subhead">Operating-point Overlay</h4>
            <FieldRow label="Significant digits (voltages):">{spin(d.vPrecision, "Significant digits (voltages)", (v) => page.edit((x) => ({ ...x, vPrecision: v })))}</FieldRow>
            <FieldRow label="Range (voltages):">{range(d.vRange, V_RANGES, "Range (voltages)", (v) => page.edit((x) => ({ ...x, vRange: v })))}</FieldRow>
            <FieldRow label="Significant digits (currents):">{spin(d.iPrecision, "Significant digits (currents)", (v) => page.edit((x) => ({ ...x, iPrecision: v })))}</FieldRow>
            <FieldRow label="Range (currents):">{range(d.iRange, I_RANGES, "Range (currents)", (v) => page.edit((x) => ({ ...x, iRange: v })))}</FieldRow>
          </div>
        </div>
      )}
    </PageFrame>
  );
}
