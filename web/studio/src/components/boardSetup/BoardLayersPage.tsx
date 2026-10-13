// Board Setup > Board Stackup > Board Editor Layers -- port of pcbnew/dialogs/panel_setup_layers.cpp: the layers a board has besides the copper ones of the Physical Stackup page
// (the technical layers KiCad starts a board with, and up to 45 user layers), the name each layer is shown by, and the type of the copper layers (signal, power plane, mixed,
// jumper -- what Freerouting and other external routers read) and of the user layers (auxiliary, or off-board at the front or the back, which flips with the board).
// The rows are in the order of KiCad's dialog; the courtyards, the outline and the margin are always on, and a copper layer's box is the stackup's. The checks are the panel's
// (`testLayerNames`); Apply sends the whole page as one undoable command (`set_board_layers`), refused with the layer's name when a layer that holds items is turned off.
import { useEffect, useState } from "react";
import { useStudioState } from "../../state/store";
import { availableUserLayers, checkLayerDraft, itemsPerLayer, layerDraft, layerDraftToSend, layerRows, sameLayerSetup, withLayerChecked, withUserLayer, LAYER_TYPE_LABEL, type LayerDraft } from "../../kicad-port/boardLayers";
import type { BoardLayerType } from "../../api/types";
import { PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

export function BoardLayersPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const board = state.board;
  const copper = board?.layers ?? [];
  const setup = board?.board_rules?.layer_setup;
  const page = usePageDraft<LayerDraft>(board?.board_rules && setup ? layerDraft(setup, copper) : undefined);
  const apply = usePageApply();
  const [toAdd, setToAdd] = useState("");
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const draft = page.draft;
  const check = draft ? checkLayerDraft(draft, copper) : null;
  const rows = draft ? layerRows(draft, copper) : [];
  const items = itemsPerLayer(board?.drawings);
  // A layer that is on now and would be off after Apply while something sits on it: the backend refuses it, and the page says so beside the layer.
  const holding = (layer: string, checked: boolean): number => (!checked && (setup?.enabled ?? []).includes(layer) ? (items[layer] ?? 0) : 0);

  const edit = (fn: (d: LayerDraft) => LayerDraft) => page.edit(fn);
  const available = draft ? availableUserLayers(draft) : [];
  const next = toAdd && available.includes(toAdd) ? toAdd : (available[0] ?? "");

  const onApply = () => {
    if (!draft || !check) return;
    if (Object.keys(check.errors).length > 0 || check.page) {
      apply.setMessage({ text: check.page ?? "Fix the layers marked in red first.", bad: true });
      return;
    }
    const stuck = rows.find((r) => holding(r.layer, r.checked) > 0);
    if (stuck) {
      apply.setMessage({ text: `${holding(stuck.layer, stuck.checked)} item(s) are on ${stuck.name}: move or delete them before the layer is turned off.`, bad: true });
      return;
    }
    const send = layerDraftToSend(draft, copper);
    if (sameLayerSetup(send, setup)) {
      apply.setMessage({ text: "Nothing to apply.", bad: false });
      page.markClean();
      return;
    }
    void apply.run({ op: "set_board_layers", settings: send }, page.markClean);
  };

  return (
    <PageFrame title="Board Editor Layers" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!draft || !check ? (
        <p className="bs-note">This backend does not report the board's layers; update the eda binary.</p>
      ) : (
        <>
          <p className="bs-note">
            The layers this board has, the name each is shown by and the type of the copper and user layers. A layer that is turned off is not offered by the layer lists and takes no new items; the copper layers are set on the Physical Stackup page.
          </p>
          <div className="bs-scroll-x">
            <table className="bs-grid" aria-label="Board layers">
              <thead>
                <tr>
                  <th>Enabled</th>
                  <th>Layer name</th>
                  <th>Layer type</th>
                  <th>In the board file</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => {
                  const error = check.errors[r.layer];
                  const held = holding(r.layer, r.checked);
                  return (
                    <tr key={r.layer} data-layer={r.layer}>
                      <td>
                        <input
                          type="checkbox"
                          aria-label={`Enable ${r.layer}`}
                          checked={r.checked}
                          disabled={r.locked}
                          title={r.lockedBecause ?? r.tip}
                          onChange={(e) => edit((d) => withLayerChecked(d, r.layer, e.target.checked))}
                        />
                      </td>
                      <td>
                        <input
                          type="text"
                          className={`bs-name${error ? " bs-invalid" : ""}`}
                          aria-label={`${r.layer} name`}
                          title={error ?? "Layer Name"}
                          value={r.name}
                          spellCheck={false}
                          onChange={(e) => edit((d) => ({ ...d, names: { ...d.names, [r.layer]: e.target.value } }))}
                        />
                      </td>
                      <td>
                        {r.typeChoices.length > 0 ? (
                          <select aria-label={`${r.layer} type`} title={r.tip} value={r.type ?? ""} onChange={(e) => edit((d) => ({ ...d, types: { ...d.types, [r.layer]: e.target.value as BoardLayerType } }))}>
                            {r.typeChoices.map((t) => (
                              <option key={t} value={t}>
                                {LAYER_TYPE_LABEL[t]}
                              </option>
                            ))}
                          </select>
                        ) : (
                          <span className="bs-note">{r.note}</span>
                        )}
                      </td>
                      <td>
                        <code>{r.layer}</code>
                      </td>
                      <td className="bs-error">{error ?? (held > 0 ? `${held} item(s) are on this layer: move or delete them first.` : "")}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
          <div className="bs-toolbar">
            <select aria-label="User-defined layer to add" value={next} disabled={available.length === 0} onChange={(e) => setToAdd(e.target.value)}>
              {available.map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
            <button
              disabled={available.length === 0}
              title={available.length === 0 ? "All user-defined layers have already been added." : "Add the selected user-defined layer to the board"}
              onClick={() => {
                if (next) edit((d) => withUserLayer(d, next));
                setToAdd("");
              }}
            >
              Add User Defined Layer
            </button>
          </div>
          {check.page && <p className="bs-error">{check.page}</p>}
        </>
      )}
    </PageFrame>
  );
}
