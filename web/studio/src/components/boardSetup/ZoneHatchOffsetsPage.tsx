// Board Setup > Board Stackup > Zone Hatch Offsets -- port of pcbnew/dialogs/panel_setup_zone_hatch_offsets.cpp: a row for each copper layer with the offset (x, y) of the pattern of
// the hatched zones on it (`ZONE_LAYER_PROPERTIES::hatching_offset`; the filler moves the pattern by the offset modulo its pitch). Apply sends the layers that have one as one undoable
// command (`set_zone_hatch_offsets`).
import { useEffect } from "react";
import { useStudioState } from "../../state/store";
import { hatchProblems, hatchRows, hatchToSend, sameHatchOffsets, type HatchRow } from "../../kicad-port/setupPages";
import { LengthField, PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

export function ZoneHatchOffsetsPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const units = state.units;
  const copper = state.board?.layers ?? [];
  const stored = state.board?.board_rules?.zone_hatch_offsets;
  const page = usePageDraft<HatchRow[]>(state.board?.board_rules && stored !== undefined ? hatchRows(copper, stored) : undefined);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const rows = page.draft;
  const problems = rows ? hatchProblems(rows) : {};
  const setRow = (layer: string, patch: Partial<HatchRow>) => page.edit((d) => d.map((r) => (r.layer === layer ? { ...r, ...patch } : r)));

  const onApply = () => {
    if (!rows) return;
    if (Object.keys(problems).length > 0) {
      apply.setMessage({ text: "Fix the offsets marked in red first.", bad: true });
      return;
    }
    const offsets = hatchToSend(rows);
    if (sameHatchOffsets(offsets, stored)) {
      apply.setMessage({ text: "Nothing to apply.", bad: false });
      page.markClean();
      return;
    }
    void apply.run({ op: "set_zone_hatch_offsets", offsets }, page.markClean);
  };

  return (
    <PageFrame title="Zone Hatch Offsets" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!rows ? (
        <p className="bs-note">This backend does not report the board's hatch offsets; update the eda binary.</p>
      ) : (
        <>
          <p className="bs-note">
            The pattern of a hatched zone starts where the board's origin puts it. An offset moves it, layer by layer, so that the webs of the zones on two layers do not lie over each other. The offset counts modulo the pattern's pitch.
          </p>
          <div className="bs-scroll-x">
            <table className="bs-grid" aria-label="Zone hatch offsets">
              <thead>
                <tr>
                  <th>Layer</th>
                  <th>Hatch offset X</th>
                  <th>Hatch offset Y</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.layer} data-layer={r.layer}>
                    <td>{r.layer}</td>
                    <td>
                      <LengthField ariaLabel={`${r.layer} hatch offset X`} units={units} value={r.x_um} invalid={!!problems[r.layer]} width={84} onChange={(v) => setRow(r.layer, { x_um: v ?? Number.NaN })} />
                    </td>
                    <td>
                      <LengthField ariaLabel={`${r.layer} hatch offset Y`} units={units} value={r.y_um} invalid={!!problems[r.layer]} width={84} onChange={(v) => setRow(r.layer, { y_um: v ?? Number.NaN })} />
                    </td>
                    <td className="bs-error">{problems[r.layer] ?? ""}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </PageFrame>
  );
}
