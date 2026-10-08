// Board Setup > Text & Graphics > Defaults -- port of pcbnew/dialogs/panel_setup_text_and_graphics.cpp (the grid of "Default Properties for
// New Graphics and Text"). The values are the `defaults.*` keys of the derived `.kicad_pro`; Apply checks them like TransferDataFromWindow
// does (line width, text size, text thickness against the size) and sends the whole grid (`set_text_graphics_defaults`).
import { useEffect } from "react";
import { useStudioState } from "../../state/store";
import { TEXT_GRAPHICS_ROWS, textRowOf, validateTextGraphics, withLineWidth, withTextFields } from "../../kicad-port/boardSetupRules";
import type { LayerClassDefaults, TextGraphicsDefaults } from "../../api/types";
import { LengthField, PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

export function TextGraphicsPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const units = state.units;
  const page = usePageDraft<TextGraphicsDefaults>(state.board?.board_rules?.text_graphics);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const draft = page.draft;
  const errors = draft ? validateTextGraphics(draft) : {};

  const onApply = () => {
    if (!draft) return;
    if (Object.keys(errors).length > 0) {
      apply.setMessage({ text: "Fix the rows marked in red first.", bad: true });
      return;
    }
    void apply.run({ op: "set_text_graphics_defaults", settings: draft }, page.markClean);
  };

  return (
    <PageFrame title="Defaults" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!draft ? (
        <p className="bs-note">This backend does not report the board's text and graphics defaults; update the eda binary.</p>
      ) : (
        <>
          <div className="bs-section">Default Properties for New Graphics and Text</div>
          <div className="bs-scroll-x">
            <table className="bs-grid">
              <thead>
                <tr>
                  <th />
                  <th>Line Thickness</th>
                  <th>Text Width</th>
                  <th>Text Height</th>
                  <th>Text Thickness</th>
                  <th>Italic</th>
                  <th>Keep Upright</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {TEXT_GRAPHICS_ROWS.map((row) => {
                  const values = textRowOf(draft, row.id);
                  const bad = errors[row.id];
                  const text = row.hasText ? (values as LayerClassDefaults) : null;
                  const textId = row.id as "silk" | "copper" | "fab" | "others";
                  const setText = (patch: Partial<LayerClassDefaults>) => page.edit((d) => withTextFields(d, textId, patch));
                  const cell = (value: number, set: (v: number) => void, label: string) => (
                    <td>
                      <LengthField ariaLabel={`${row.label} ${label}`} units={units} value={value} invalid={!!bad} width={74} onChange={(v) => set(v ?? Number.NaN)} />
                    </td>
                  );
                  return (
                    <tr key={row.id}>
                      <th scope="row">{row.label}</th>
                      {cell(values.line_width_um, (v) => page.edit((d) => withLineWidth(d, row.id, v)), "Line Thickness")}
                      {text ? cell(text.text_width_um, (v) => setText({ text_width_um: v }), "Text Width") : <td />}
                      {text ? cell(text.text_height_um, (v) => setText({ text_height_um: v }), "Text Height") : <td />}
                      {text ? cell(text.text_thickness_um, (v) => setText({ text_thickness_um: v }), "Text Thickness") : <td />}
                      <td>{text && <input type="checkbox" aria-label={`${row.label} Italic`} checked={text.italic} onChange={(e) => setText({ italic: e.target.checked })} />}</td>
                      <td>{text && <input type="checkbox" aria-label={`${row.label} Keep Upright`} checked={text.upright} onChange={(e) => setText({ upright: e.target.checked })} />}</td>
                      <td className="bs-error">{bad ?? ""}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
          <p className="bs-note">Sizes are in {units}. The board outline and the courtyards have a line thickness only.</p>
        </>
      )}
    </PageFrame>
  );
}
