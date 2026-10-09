// Board Setup > Board Stackup > Physical Stackup (+ Board Finish) -- port of pcbnew/board_stackup_manager/panel_board_stackup.cpp and
// panel_board_finish.cpp, reduced to what the model holds: the number of copper layers, each layer's type, material and thickness, the
// dielectric constant and loss tangent of the dielectrics, and the finish. The table is the `(stackup ..)` of the derived `.kicad_pcb`;
// the board thickness is what the layers add up to (`BuildBoardThicknessFromStackup`), and the copper layers are the board's layers.
//
// Colours: a mask, a silkscreen and a dielectric carry KiCad's colour name (or a typed `#RRGGBB`), which the 3D viewer's "Use board stackup colors" paints the board
// with (kicad-port/appearance3d.ts). Not ported: sub-layers of a dielectric, "Add/Remove Dielectric Layer...", "Adjust Dielectric Thickness" and the impedance-
// controlled switch. A different copper layer count rebuilds the table from the default stackup, keeping what the layers that remain say
// (`withCopperLayers`); reducing it is refused up front while a track, via or zone is still on a layer that would go.
import { useEffect, useMemo } from "react";
import { useStudioState } from "../../state/store";
import {
  copperLayersLost,
  defaultStackup,
  isDielectric,
  isMaterialEditable,
  isThicknessEditable,
  mmText,
  stackupThicknessUm,
  validateStackup,
  withCopperLayers,
  withKnownKinds,
} from "../../kicad-port/boardSetupRules";
import { NOT_SPECIFIED, STACKUP_COLOR_NAMES, parseColor, stackupColorKind, toRgbHex } from "../../kicad-port/appearance3d";
import type { StackupLayer, StackupSettings } from "../../api/types";
import { Check, FieldRow, NumberField, PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

const COPPER_COUNTS = Array.from({ length: 16 }, (_, i) => (i + 1) * 2);

/** `GetStandardCopperFinishes`: the standard names (the ones a `.gbrjob` file knows); "Not specified" writes none. */
const COPPER_FINISHES = ["ENIG", "ENEPIG", "HAL SnPb", "HAL lead-free", "Hard gold", "Immersion tin", "Immersion nickel", "Immersion silver", "Immersion gold", "HT_OSP", "OSP", "None"];

const EDGE_CONNECTORS = ["No", "Yes", "Yes, bevelled"];

const USER_DEFINED = "User defined";

/** The colour cell of a stackup row (`PANEL_BOARD_STACKUP`'s colour combo): KiCad's names for the item's kind, "User defined" for a typed `#RRGGBB`. */
function StackupColor({ kind, value, ariaLabel, onChange }: { kind: "silk" | "mask" | "dielectric"; value: string | undefined; ariaLabel: string; onChange: (color: string | undefined) => void }) {
  const names: readonly string[] = STACKUP_COLOR_NAMES[kind];
  const custom = value !== undefined && value.startsWith("#");
  const selected = custom ? USER_DEFINED : (value ?? NOT_SPECIFIED);
  return (
    <span style={{ display: "inline-flex", gap: 4, alignItems: "center" }}>
      <select
        aria-label={ariaLabel}
        value={selected}
        onChange={(e) => {
          const v = e.target.value;
          onChange(v === NOT_SPECIFIED ? undefined : v === USER_DEFINED ? (custom ? value : "#808080") : v);
        }}
      >
        {names.map((n) => (
          <option key={n} value={n}>
            {n}
          </option>
        ))}
        {!custom && value !== undefined && !names.includes(value) && <option value={value}>{value}</option>}
        <option value={USER_DEFINED}>{USER_DEFINED}</option>
      </select>
      {custom && <input type="color" aria-label={`${ariaLabel} (user defined)`} value={toRgbHex(parseColor(value!) ?? { r: 0.5, g: 0.5, b: 0.5, a: 1 })} onChange={(e) => onChange(e.target.value)} style={{ width: 22, height: 18, padding: 0 }} />}
    </span>
  );
}

export function StackupPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const board = state.board;
  const rules = board?.board_rules;
  const source = useMemo<StackupSettings | undefined>(() => {
    if (!rules) return undefined;
    const copper = rules.copper_layers ?? rules.layer_names?.length ?? board?.layers.length ?? 2;
    return { copper_layers: copper, stackup: withKnownKinds(rules.stackup ?? defaultStackup(copper, rules.board_thickness_um ?? 1600)) };
  }, [rules, board?.layers.length]);
  const page = usePageDraft<StackupSettings>(source);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const draft = page.draft;
  const problem = draft ? validateStackup(draft) : null;
  const lost = draft && board ? copperLayersLost(board.routing, draft.copper_layers) : [];

  const setLayer = (name: string, kind: string | undefined, patch: Partial<StackupLayer>) =>
    page.edit((d) => ({ ...d, stackup: { ...d.stackup, layers: d.stackup.layers.map((l) => (l.name === name && l.kind === kind ? { ...l, ...patch } : l)) } }));
  const setStackup = (patch: Partial<StackupSettings["stackup"]>) => page.edit((d) => ({ ...d, stackup: { ...d.stackup, ...patch } }));

  const onApply = () => {
    if (!draft) return;
    if (problem) {
      apply.setMessage({ text: problem, bad: true });
      return;
    }
    if (lost.length > 0) {
      apply.setMessage({ text: `${lost.map((l) => l.layer).join(", ")} still carry copper: move or delete it before reducing the number of layers.`, bad: true });
      return;
    }
    void apply.run({ op: "set_stackup", settings: draft }, page.markClean);
  };

  return (
    <PageFrame title="Physical Stackup" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!draft ? (
        <p className="bs-note">The board is not loaded.</p>
      ) : (
        <>
          <FieldRow label="Copper layers:" title="Select the number of copper layers in the stackup">
            <select aria-label="Copper layers" value={draft.copper_layers} onChange={(e) => page.edit((d) => withCopperLayers(d, Number(e.target.value)))}>
              {COPPER_COUNTS.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </FieldRow>
          {lost.length > 0 && (
            <p className="bs-error">
              {lost.map((l) => `${l.layer} (${l.items} item${l.items === 1 ? "" : "s"})`).join(", ")} would go with this many layers, and still carry copper.
            </p>
          )}

          <div className="bs-scroll-x">
            <table className="bs-grid">
              <thead>
                <tr>
                  <th>Layer</th>
                  <th>Type</th>
                  <th>Material</th>
                  <th>Color</th>
                  <th>Thickness (mm)</th>
                  <th>Epsilon R</th>
                  <th>Loss Tan</th>
                </tr>
              </thead>
              <tbody>
                {draft.stackup.layers.map((l, i) => {
                  const edit = (patch: Partial<StackupLayer>) => setLayer(l.name, l.kind, patch);
                  const badThickness = l.thickness_mm !== null && (!Number.isFinite(l.thickness_mm) || l.thickness_mm < 0);
                  return (
                    <tr key={`${i}:${l.name}`}>
                      <td>{l.name}</td>
                      <td>{l.kind ?? ""}</td>
                      <td>
                        {isMaterialEditable(l.kind) ? (
                          <input type="text" aria-label={`${l.name} material`} value={l.material ?? ""} onChange={(e) => edit({ material: e.target.value === "" ? null : e.target.value })} style={{ width: 110 }} />
                        ) : (
                          (l.material ?? "")
                        )}
                      </td>
                      <td>
                        {stackupColorKind(l.kind) && (
                          <StackupColor kind={stackupColorKind(l.kind)!} value={l.color} ariaLabel={`${l.name} color`} onChange={(color) => (color === undefined ? edit({ color: undefined }) : edit({ color }))} />
                        )}
                      </td>
                      <td>
                        {isThicknessEditable(l.kind) && l.thickness_mm !== null ? (
                          <NumberField ariaLabel={`${l.name} thickness`} width={72} value={l.thickness_mm} invalid={badThickness} onChange={(v) => edit({ thickness_mm: v ?? Number.NaN })} />
                        ) : (
                          (l.thickness_mm ?? "")
                        )}
                      </td>
                      <td>
                        {isDielectric(l.kind) && (
                          <NumberField
                            ariaLabel={`${l.name} epsilon r`}
                            width={54}
                            optional
                            value={l.epsilon_r}
                            onChange={(v) => setLayer(l.name, l.kind, v === undefined ? { epsilon_r: undefined } : { epsilon_r: v })}
                          />
                        )}
                      </td>
                      <td>
                        {isDielectric(l.kind) && (
                          <NumberField
                            ariaLabel={`${l.name} loss tangent`}
                            width={54}
                            optional
                            value={l.loss_tangent}
                            onChange={(v) => setLayer(l.name, l.kind, v === undefined ? { loss_tangent: undefined } : { loss_tangent: v })}
                          />
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
          <div className="bs-stack-total" title="BuildBoardThicknessFromStackup: every layer's thickness added up">
            Board thickness from stackup: {mmText(stackupThicknessUm(draft.stackup))} mm
          </div>

          <div className="bs-section">Board Finish</div>
          <FieldRow label="Copper finish:">
            <select aria-label="Copper finish" value={draft.stackup.copper_finish ?? ""} onChange={(e) => setStackup({ copper_finish: e.target.value === "" ? undefined : e.target.value })}>
              <option value="">Not specified</option>
              {COPPER_FINISHES.map((f) => (
                <option key={f} value={f}>
                  {f}
                </option>
              ))}
              {draft.stackup.copper_finish && !COPPER_FINISHES.includes(draft.stackup.copper_finish) && <option value={draft.stackup.copper_finish}>{draft.stackup.copper_finish}</option>}
            </select>
          </FieldRow>
          <Check checked={draft.stackup.edge_plating ?? false} onChange={(v) => setStackup({ edge_plating: v })}>
            Plated board edge
          </Check>
          <FieldRow label="Edge card connectors:" title="Options for edge card connectors.">
            <select aria-label="Edge card connectors" value={draft.stackup.edge_connector ?? 0} onChange={(e) => setStackup({ edge_connector: Number(e.target.value) })}>
              {EDGE_CONNECTORS.map((label, i) => (
                <option key={label} value={i}>
                  {label}
                </option>
              ))}
            </select>
          </FieldRow>
        </>
      )}
    </PageFrame>
  );
}
