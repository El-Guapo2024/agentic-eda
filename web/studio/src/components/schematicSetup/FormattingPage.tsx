// Schematic Setup > General > Formatting -- port of eeschema/dialogs/panel_setup_formatting.cpp, for the one setting of that page the design keeps: Connection grid spacing
// (`SCHEMATIC_SETTINGS::m_ConnectionGridSize`, in mils, from `MIN_CONNECTION_GRID_MILS` to 10000: `TransferDataFromWindow` validates it). It is the grid ERC's
// `endpoint_off_grid` expects wire ends and pins on; the derived project carries it to kicad-cli as `schematic.connection_grid_size`, and a marker menu's "Edit connection
// grid spacing..." (`ID_EDIT_CONNECTION_GRID`) opens this page. The page's other settings (text sizes, line widths, junction and hop-over sizes, intersheet references, the
// operating-point display) have nothing in the model to edit yet, so they are not shown as dead fields. Apply is one undoable command (`set_connection_grid`).
import { useCallback, useEffect, useState } from "react";
import { fetchSchFormatting } from "../../api/client";
import type { SchFormattingReply } from "../../api/types";
import { useStudioState } from "../../state/store";
import { FieldRow, NumberField, PageFrame, usePageApply, usePageDraft, type PageProps } from "../boardSetup/fields";

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

  const page = usePageDraft<{ grid: number | undefined }>(info ? { grid: info.connection_grid_mils } : undefined);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);

  const min = info?.min_connection_grid_mils ?? 25;
  const max = info?.max_connection_grid_mils ?? 10000;
  const grid = page.draft?.grid;
  // `m_connectionGridSize.Validate( MIN_CONNECTION_GRID_MILS, 10000, MILS )`; the field keeps whole mils.
  const error = grid === undefined || Number.isNaN(grid) ? "Enter a number of mils." : !Number.isInteger(grid) ? "Whole mils only." : grid < min || grid > max ? `Connection grid spacing must be from ${min} to ${max} mils.` : null;

  const onApply = () => {
    if (error !== null || grid === undefined) return;
    void apply.run({ op: "set_connection_grid", mils: grid }, () => {
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
        </>
      )}
    </PageFrame>
  );
}
