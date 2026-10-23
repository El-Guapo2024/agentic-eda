// Schematic Setup > General > Formatting -- port of eeschema/dialogs/panel_setup_formatting.cpp, for the one setting of that page the design keeps: Connection grid spacing
// (`SCHEMATIC_SETTINGS::m_ConnectionGridSize`, in mils, from `MIN_CONNECTION_GRID_MILS` to 10000: `TransferDataFromWindow` validates it). It is the grid ERC's
// `endpoint_off_grid` expects wire ends and pins on; the derived project carries it to kicad-cli as `schematic.connection_grid_size`, and a marker menu's "Edit connection
// grid spacing..." (`ID_EDIT_CONNECTION_GRID`) opens this page. And Inter-sheet References (`m_IntersheetRefs*`: shown, own page, Standard (1,2,3) or Abbreviated (1..3),
// prefix and suffix; `PANEL_SETUP_FORMATTING::onCheckBoxIref` greys the others out while they are not shown): how a global label's Sheet References read, in the derived
// project as `schematic.drawing.intersheets_ref_*`. The page's other settings (text sizes, line widths, junction and hop-over sizes, the operating-point display) have
// nothing in the model to edit yet, so they are not shown as dead fields. Apply is one undoable command (`set_connection_grid`, `set_intersheet_refs`, both in one
// `batch` when both changed).
import { useCallback, useEffect, useState } from "react";
import { fetchSchFormatting } from "../../api/client";
import type { Cmd, IntersheetRefs, SchFormattingReply } from "../../api/types";
import { useStudioState } from "../../state/store";
import { Check, FieldRow, NumberField, PageFrame, usePageApply, usePageDraft, type PageProps } from "../boardSetup/fields";

/** `SCHEMATIC_SETTINGS`' defaults (`DEFAULT_IREF_PREFIX`, `DEFAULT_IREF_SUFFIX`): what an older backend that sends none means. */
const IREF_DEFAULT: IntersheetRefs = { show: false, own_page: true, short: false, prefix: "[", suffix: "]" };

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

  const page = usePageDraft<{ grid: number | undefined; iref: IntersheetRefs }>(info ? { grid: info.connection_grid_mils, iref: info.intersheet_refs ?? IREF_DEFAULT } : undefined);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);

  const min = info?.min_connection_grid_mils ?? 25;
  const max = info?.max_connection_grid_mils ?? 10000;
  const grid = page.draft?.grid;
  // `m_connectionGridSize.Validate( MIN_CONNECTION_GRID_MILS, 10000, MILS )`; the field keeps whole mils.
  const error = grid === undefined || Number.isNaN(grid) ? "Enter a number of mils." : !Number.isInteger(grid) ? "Whole mils only." : grid < min || grid > max ? `Connection grid spacing must be from ${min} to ${max} mils.` : null;

  const iref = page.draft?.iref ?? IREF_DEFAULT;
  const setIref = (change: Partial<IntersheetRefs>) => page.edit((d) => ({ ...d, iref: { ...d.iref, ...change } }));
  const onApply = () => {
    if (error !== null || grid === undefined) return;
    const cmds: Cmd[] = [];
    if (grid !== info?.connection_grid_mils) cmds.push({ op: "set_connection_grid", mils: grid });
    if (JSON.stringify(iref) !== JSON.stringify(info?.intersheet_refs ?? IREF_DEFAULT)) cmds.push({ op: "set_intersheet_refs", settings: iref });
    if (cmds.length === 0) {
      page.markClean();
      return;
    }
    void apply.run(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds }, () => {
      page.markClean();
      void load();
    });
  };

  return (
    <PageFrame title="Formatting" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? problem ?? error} bad={apply.message?.bad || problem !== null || (page.dirty && error !== null)} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!page.draft ? (
        <p className="bs-note">{problem ?? "Reading the schematic's formatting…"}</p>
      ) : (
        <>
          <p className="bs-note">
            The grid wire ends and symbol pins are expected to be on. ERC reports the ones that are not (&ldquo;Symbol pin or wire end off connection grid&rdquo;). KiCad&rsquo;s default is 50 mils.
          </p>
          <FieldRow label="Connection grid spacing:" unit="mils" error={page.dirty ? error : null} title="SCHEMATIC_SETTINGS::m_ConnectionGridSize">
            <NumberField value={grid} onChange={(v) => page.edit((d) => ({ ...d, grid: v }))} invalid={error !== null} ariaLabel="Connection grid spacing in mils" width={80} />
          </FieldRow>
          <h4 className="bs-subhead">Inter-sheet References</h4>
          <Check checked={iref.show} onChange={(v) => setIref({ show: v })} title="SCHEMATIC_SETTINGS::m_IntersheetRefsShow">
            Show inter-sheet references
          </Check>
          <Check checked={iref.own_page} disabled={!iref.show} onChange={(v) => setIref({ own_page: v })} title="m_IntersheetRefsListOwnPage">
            Show own page reference
          </Check>
          <label className="bs-check">
            <input type="radio" name="iref-format" aria-label="Standard (1,2,3)" checked={!iref.short} disabled={!iref.show} onChange={() => setIref({ short: false })} /> Standard (1,2,3)
          </label>
          <label className="bs-check">
            <input type="radio" name="iref-format" aria-label="Abbreviated (1..3)" checked={iref.short} disabled={!iref.show} onChange={() => setIref({ short: true })} /> Abbreviated (1..3)
          </label>
          <FieldRow label="Prefix:">
            <input aria-label="Inter-sheet references prefix" value={iref.prefix} disabled={!iref.show} onChange={(e) => setIref({ prefix: e.target.value })} style={{ width: 160 }} />
          </FieldRow>
          <FieldRow label="Suffix:">
            <input aria-label="Inter-sheet references suffix" value={iref.suffix} disabled={!iref.show} onChange={(e) => setIref({ suffix: e.target.value })} style={{ width: 160 }} />
          </FieldRow>
        </>
      )}
    </PageFrame>
  );
}
