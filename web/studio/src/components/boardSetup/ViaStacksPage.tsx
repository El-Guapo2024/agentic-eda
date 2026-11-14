// Board Setup > Design Rules > Microvia Stacks -- port of KiCad 25ec2335 pcbnew/dialogs/panel_setup_via_stacks.cpp (`PANEL_SETUP_VIA_STACKS`) and
// dialog_microvia_stack.cpp (`DIALOG_MICROVIA_STACK`): the board's microvia stack presets (`BOARD_DESIGN_SETTINGS::m_ViaStackPresets`, the project's
// `via_stack_presets`) in a grid -- Name (edited in place), Type, Layers, Diameter / Hole, Fill, Pitch, the rest read only -- with Add... (the name first, "New
// Microvia Stack Preset", then the Microvia Stack dialog), Edit... (the dialog on the row) and Remove. Apply sends the list as one undoable
// `set_via_stack_presets`; "Import Settings from Another Board..." brings it with the pre-defined sizes (`m_TracksAndViasOpt`), as KiCad's does (UI-LEFT-3 B3-8).
//
// The board's `(generated (type via_stack) ..)` items are kept as KiCad wrote them (written back with their members); nothing here places or edits one.
import { useEffect, useMemo, useState } from "react";
import { useStudioState } from "../../state/store";
import type { ViaStackPreset } from "../../api/types";
import { MAX_MICROVIA_STACK_PITCH_NM, microviaStackProblem, nameIsFree, nameTakenMessage, netclassViaSize, newViaStackPreset, presetRow, settledPreset, viaStacksProblem } from "../../kicad-port/viaStacks";
import { layerDisplayName } from "../../kicad-port/boardLayers";
import { DialogShell, lengthText } from "../pcbDialogKit";
import { MessageDialog, NameDialog } from "../panels/appearance/shared";
import { Check, FieldRow, LengthField, PageFrame, usePageApply, usePageDraft, type PageProps, useSetupImport } from "./fields";

type Dialog = { kind: "name"; initial: string } | { kind: "edit"; row: number | null; preset: ViaStackPreset } | { kind: "error"; message: string; then?: Dialog };

export function ViaStacksPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const units = state.units;
  const board = state.board;
  const rules = board?.board_rules;
  const copper = useMemo(() => (board?.layers ?? []).filter((l) => /^(F|B|In\d+)\.Cu$/.test(l)), [board?.layers]);
  const layerName = (l: string) => layerDisplayName(rules?.layer_setup, l);
  const fmt = (nm: number, withUnit: boolean) => (withUnit ? `${lengthText(nm, units)} ${units}` : lengthText(nm, units));
  const stored = board?.drawings?.via_stack_presets;
  const source = useMemo(() => (board ? (stored ?? []).map((p) => ({ ...p })) : undefined), [board, stored]);
  const page = usePageDraft<ViaStackPreset[]>(source);
  // "Import Settings from Another Board..." > "Predefined track & via dimensions" (`PANEL_SETUP_VIA_STACKS::ImportSettingsFrom`)
  useSetupImport("sizes", (b) => (b.drawings?.via_stack_presets ?? []).map((p) => ({ ...p })), page.importDraft);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const [cursor, setCursor] = useState(0);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  // the cells being typed in the Name column, kept until the cell is left (`onCellChanged` runs then)
  const [typing, setTyping] = useState<{ row: number; text: string } | null>(null);
  const presets = page.draft ?? [];
  const row = Math.min(cursor, presets.length - 1);

  /** `onCellChanged` on the Name column: trimmed; empty or taken goes back to the old name (a taken one with the error). */
  const commitName = () => {
    if (!typing) return;
    const name = typing.text.trim();
    const r = typing.row;
    setTyping(null);
    if (name === "" || !nameIsFree(presets, name, r)) {
      if (name !== "") setDialog({ kind: "error", message: nameTakenMessage(name) });
      return;
    }
    if (presets[r]?.name !== name) page.edit((d) => d.map((p, k) => (k === r ? { ...p, name } : p)));
  };

  const onApply = () => {
    if (!page.draft) return;
    const problem = viaStacksProblem(page.draft, copper, fmt(MAX_MICROVIA_STACK_PITCH_NM, true));
    if (problem) {
      setCursor(problem.row);
      apply.setMessage({ text: problem.message, bad: true });
      return;
    }
    void apply.run({ op: "set_via_stack_presets", presets: page.draft }, page.markClean);
  };

  return (
    <PageFrame title="Microvia Stacks" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!page.draft ? (
        <p className="bs-note">The board is not loaded.</p>
      ) : (
        <>
          <div className="bs-scroll-x">
            <table className="bs-grid" aria-label="Microvia stack presets">
              <thead>
                <tr>
                  {["Name", "Type", "Layers", "Diameter / Hole", "Fill", "Pitch"].map((h) => (
                    <th key={h}>{h}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {presets.map((p, r) => {
                  const cells = presetRow(p, layerName, fmt);
                  return (
                    <tr key={r} data-row={r} className={r === row ? "bs-row-current" : undefined} onClick={() => setCursor(r)} onDoubleClick={() => setDialog({ kind: "edit", row: r, preset: { ...p } })}>
                      <td>
                        <input
                          type="text"
                          aria-label={`Preset ${r + 1} name`}
                          value={typing?.row === r ? typing.text : p.name}
                          spellCheck={false}
                          onFocus={() => setCursor(r)}
                          onChange={(e) => setTyping({ row: r, text: e.target.value })}
                          onBlur={commitName}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") commitName();
                          }}
                        />
                      </td>
                      {cells.slice(1).map((c, k) => (
                        <td key={k}>{c}</td>
                      ))}
                    </tr>
                  );
                })}
                {presets.length === 0 && (
                  <tr>
                    <td colSpan={6} className="bs-note">
                      No presets.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
          <div className="bs-toolbar">
            <button onClick={() => setDialog({ kind: "name", initial: "" })}>Add...</button>
            <button disabled={row < 0} onClick={() => row >= 0 && setDialog({ kind: "edit", row, preset: { ...presets[row]! } })}>
              Edit...
            </button>
            <button
              disabled={row < 0}
              onClick={() => {
                if (row < 0) return;
                page.edit((d) => d.filter((_, k) => k !== row));
              }}
            >
              Remove
            </button>
          </div>

          {dialog?.kind === "name" && (
            <NameDialog
              title="New Microvia Stack Preset"
              label="Preset name:"
              initial={dialog.initial}
              onCancel={() => setDialog(null)}
              onOk={(typed) => {
                const name = typed.trim();
                if (name === "") return setDialog(null);
                // `nameIsFree`, else the error and the name asked again
                if (!nameIsFree(presets, name, -1)) return setDialog({ kind: "error", message: nameTakenMessage(name), then: { kind: "name", initial: name } });
                setDialog({ kind: "edit", row: null, preset: newViaStackPreset(name) });
              }}
            />
          )}
          {dialog?.kind === "edit" && (
            <MicroviaStackDialog
              preset={dialog.preset}
              copper={copper}
              layerName={layerName}
              units={units}
              netclassSize={netclassViaSize(rules?.default_class, { diameter: rules?.via_diameter ?? 0, drill: rules?.via_drill ?? 0 })}
              onCancel={() => setDialog(null)}
              onOk={(p) => {
                // `editPreset`: the dialog's preset under the row's name
                const target = dialog.row;
                if (target === null) {
                  page.edit((d) => [...d, p]);
                  setCursor(presets.length);
                } else page.edit((d) => d.map((q, k) => (k === target ? { ...p, name: q.name } : q)));
                setDialog(null);
              }}
            />
          )}
          {dialog?.kind === "error" && <MessageDialog title="Error" message={dialog.message} onOk={() => setDialog(dialog.then ?? null)} onCancel={() => setDialog(dialog.then ?? null)} />}
        </>
      )}
    </PageFrame>
  );
}

/**
 * `DIALOG_MICROVIA_STACK` on a preset (`PANEL_SETUP_VIA_STACKS::editPreset`'s `PCB_VIA_STACK temp` with the preset applied): Start layer, End layer (the board's
 * copper layers), Type (Stacked, Staggered), Via diameter and Via hole, "Use netclass values" (the fields then show the Default class's microvia sizes and are
 * greyed, `showNetclassViaSize`; unticked again they show what was typed, a 0 giving way to the class's), Copper-filled (on and greyed for a stacked stack), Capped,
 * and Pitch (a staggered stack's only). OK runs `TransferDataFromWindow`'s checks with their messages.
 */
function MicroviaStackDialog({
  preset,
  copper,
  layerName,
  units,
  netclassSize,
  onOk,
  onCancel,
}: {
  preset: ViaStackPreset;
  copper: readonly string[];
  layerName: (l: string) => string;
  units: "mm" | "mil" | "in";
  netclassSize: { size: number; drill: number };
  onOk: (p: ViaStackPreset) => void;
  onCancel: () => void;
}) {
  const [p, setP] = useState<ViaStackPreset>(preset);
  // `m_explicitViaSize` / `m_explicitViaDrill`: what the fields hold while "Use netclass values" shows the class's
  const [explicit, setExplicit] = useState({ size: preset.via_size_nm, drill: preset.via_drill_nm });
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<ViaStackPreset>) => setP((x) => ({ ...x, ...patch }));
  const shown = p.use_netclass ? netclassSize : { size: p.via_size_nm, drill: p.via_drill_nm };
  const layerSelect = (label: string, value: string, onChange: (v: string) => void) => (
    <select aria-label={label} value={value} onChange={(e) => onChange(e.target.value)}>
      {!copper.includes(value) && <option value={value}>{layerName(value)}</option>}
      {copper.map((l) => (
        <option key={l} value={l}>
          {layerName(l)}
        </option>
      ))}
    </select>
  );
  const ok = () => {
    const why = microviaStackProblem(p, copper, `${lengthText(MAX_MICROVIA_STACK_PITCH_NM, units)} ${units}`);
    if (why) return setError(why);
    onOk(settledPreset(p));
  };
  return (
    <>
      <DialogShell title="Microvia Stack" width={380} onCancel={onCancel} onOk={ok}>
        <FieldRow label="Start layer:">{layerSelect("Start layer", p.start_layer, (v) => set({ start_layer: v }))}</FieldRow>
        <FieldRow label="End layer:">{layerSelect("End layer", p.end_layer, (v) => set({ end_layer: v }))}</FieldRow>
        <FieldRow label="Type:">
          <select aria-label="Type" value={p.staggered ? 1 : 0} onChange={(e) => set({ staggered: e.target.value === "1", ...(e.target.value === "1" ? {} : { filled: true }) })}>
            <option value={0}>Stacked</option>
            <option value={1}>Staggered</option>
          </select>
        </FieldRow>
        <FieldRow label="Via diameter:">
          <LengthField ariaLabel="Via diameter" units={units} value={shown.size} disabled={p.use_netclass} width={90} onChange={(v) => set({ via_size_nm: v ?? Number.NaN })} />
          <span className="bs-unit">{units}</span>
        </FieldRow>
        <FieldRow label="Via hole:">
          <LengthField ariaLabel="Via hole" units={units} value={shown.drill} disabled={p.use_netclass} width={90} onChange={(v) => set({ via_drill_nm: v ?? Number.NaN })} />
          <span className="bs-unit">{units}</span>
        </FieldRow>
        <Check
          checked={p.use_netclass}
          onChange={(on) => {
            // `onUseNetclass`: on keeps what was typed and shows the class's; off puts back what was typed, a 0 (never typed) giving way to the class's
            if (on) {
              setExplicit({ size: p.via_size_nm, drill: p.via_drill_nm });
              set({ use_netclass: true });
            } else set({ use_netclass: false, via_size_nm: explicit.size > 0 ? explicit.size : netclassSize.size, via_drill_nm: explicit.drill > 0 ? explicit.drill : netclassSize.drill });
          }}
        >
          Use netclass values
        </Check>
        <Check checked={p.staggered ? p.filled : true} disabled={!p.staggered} onChange={(v) => set({ filled: v })}>
          Copper-filled
        </Check>
        <Check checked={p.capped} onChange={(v) => set({ capped: v })}>
          Capped
        </Check>
        <FieldRow label="Pitch:">
          <LengthField ariaLabel="Pitch" units={units} value={p.pitch_nm} disabled={!p.staggered} width={90} onChange={(v) => set({ pitch_nm: v ?? Number.NaN })} />
          <span className="bs-unit">{units}</span>
        </FieldRow>
      </DialogShell>
      {error !== null && <MessageDialog title="Error" message={error} onOk={() => setError(null)} onCancel={() => setError(null)} />}
    </>
  );
}
