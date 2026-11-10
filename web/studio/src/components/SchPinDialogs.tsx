// The sheet-pin dialogs: Cleanup Sheet Pins' question (`SCH_EDIT_TOOL::CleanupSheetPins`'s `IsOK`) and Sync Sheet Pins (`DIALOG_SYNC_SHEET_PINS`): for each
// sheet, its pins set against the hierarchical labels of its own file -- what matches, what differs in shape, which pins have no label and which labels
// have no pin -- with the fixes the pin side allows (add the missing pins, delete the unreferenced ones, take a label's shape).
//
// The label side's Add Labels (UI-LEFT S21): `PANEL_SYNC_SHEET_PINS::OnBtnAddLabelsClicked` -> `SHEET_SYNCHRONIZATION_AGENT::PlaceHieraLable`, which hides the
// dialog, goes into the sheet (`changeSheet`) and runs Place Hierarchical Label with the pins as templates (`PreparePlacementTemplate`): each label rides the
// cursor with its pin's text and shape and the tool's last spin, bold, italic and Auto (`TwoClickPlace`'s `GetPlacementTemplate` branch), one click each.
// When the last one is placed, or the labels in the hand are dropped, the dialog shows itself again (`TwoClickPlace`: `EndPlaceItem`, `CanPlaceMore`,
// `m_dialogSyncSheetPin->Show( true )`; at the tool's end `EndPlacement()` then `Show( true )`, UI-LEFT-2 SS15), on the sheets it had, which stay on their sheet
// path while the sheet in view is the one the labels went into: its reads and edits are addressed to that path (`on_sheet`).
// Differs: Esc drops every label still in the hand at once (KiCad's first Esc drops the one riding, the next click makes the next, the second Esc ends the tool).
// Not here: renaming or reshaping a label from the dialog.
import { useEffect, useState } from "react";
import type { SchToolDialog } from "../api/schEditTypes";
import type { Cmd, LabelShape, Sheet } from "../api/types";
import { autoplacePins, syncRows, type HierLabel } from "../kicad-port/schSheetPins";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { SchDialogShell } from "./SchDialogShell";
import { loadHierLabels, pinExtentOf } from "./schematic/schPinTool";
import { carryItem } from "../actions/schPlaceActions";
import { lastPlaced } from "../kicad-port/schLastPlaced";
import type { CarriedItem } from "../kicad-port/schPlaceTool";
import { samePath } from "../kicad-port/sheetPages";
import { getSchCarry, subscribeSchCarry } from "../state/schCarryStore";
import { fetchSchematic } from "../api/client";

export function CleanupPinsDialog({ dialog }: { dialog: Extract<SchToolDialog, { kind: "cleanup_pins" }> }) {
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  const submit = () => {
    void api.cmdBatch(dialog.pins.map((p): Cmd => ({ op: "sch_edit", verb: "delete_sheet_pin", id: p.id })));
    close();
  };
  return (
    <SchDialogShell title="Cleanup Sheet Pins" onCancel={close} onOk={submit} okLabel="Delete pins">
      <p style={{ marginTop: 0 }}>Do you wish to delete the unreferenced pins from this sheet?</p>
      <ul style={{ margin: 0, paddingLeft: 18 }}>
        {dialog.pins.map((p) => (
          <li key={p.id}>{p.name}</li>
        ))}
      </ul>
    </SchDialogShell>
  );
}

export function SyncPinsDialog({ dialog }: { dialog: Extract<SchToolDialog, { kind: "sync_pins" }> }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  // the sheet path the sheets are on: the one in view, or the one the dialog was opened on before Add Labels went into a sheet
  const path = dialog.path ?? state.currentSheetPath;
  const inView = samePath(path, state.currentSheetPath);
  // the sheets of a path not in view, read from the server (again after each edit made here)
  const [away, setAway] = useState<Sheet[] | null>(null);
  const [edits, setEdits] = useState(0);
  useEffect(() => {
    if (inView) return;
    let live = true;
    void fetchSchematic(path).then(
      (s) => live && setAway(s.sheets),
      () => live && setAway([])
    );
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [inView, path.join("/"), edits]);
  const onPath = inView ? (state.schematic?.sheets ?? []) : (away ?? []);
  const sheets = dialog.sheetIds.map((id) => onPath.find((s) => s.id === id)).filter((s): s is Sheet => s !== undefined);
  const [page, setPage] = useState(Math.max(0, dialog.sheetIds.indexOf(dialog.first ?? "")));
  const [labelsBySheet, setLabelsBySheet] = useState<Record<string, HierLabel[]>>({});

  // Read each sheet's hierarchical labels once, when the dialog opens.
  useEffect(() => {
    let live = true;
    for (const id of dialog.sheetIds) {
      void loadHierLabels(path, id)
        .then((labels) => live && setLabelsBySheet((prev) => ({ ...prev, [id]: labels })))
        .catch(() => live && setLabelsBySheet((prev) => ({ ...prev, [id]: [] })));
    }
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dialog]);

  /** An edit of the sheets' pins, addressed to their sheet path when it is not the one in view (`on_sheet`, one step). */
  const send = async (cmds: Cmd[]) => {
    if (cmds.length === 0) return;
    if (inView) return void (await api.cmdBatch(cmds));
    const one: Cmd = cmds.length === 1 ? cmds[0]! : ({ op: "batch", cmds } as unknown as Cmd);
    await api.cmd({ op: "on_sheet", sheet: path.join("/"), cmd: one } as unknown as Cmd);
    setEdits((n) => n + 1);
  };

  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  const sheet = sheets[Math.min(page, sheets.length - 1)];
  if (!sheet) return null;
  const labels = labelsBySheet[sheet.id];
  const rows = labels ? syncRows(sheet.pins, labels) : [];
  const missing = rows.filter((r) => r.kind === "label_only");
  const unreferenced = rows.filter((r) => r.kind === "pin_only");

  const addPins = (names: string[]) => {
    const wanted = (labels ?? []).filter((l) => names.includes(l.name));
    const placed = autoplacePins({ at: sheet.at, size: sheet.size }, sheet.pins, wanted, pinExtentOf);
    if (placed.length === 0) return;
    void send(placed.map((p): Cmd => ({ op: "sch_edit", verb: "add_sheet_pin", sheet: sheet.id, name: p.label.name, shape: p.label.shape ?? "passive", at: { x: p.at[0], y: p.at[1] } })));
  };
  /** Add Labels: into the sheet, a hierarchical label per pin in the hand (`PlaceHieraLable`); the dialog comes back when they are placed or dropped. */
  const addLabels = async (pins: ReadonlyArray<{ name: string; shape?: LabelShape | null }>) => {
    if (pins.length === 0) return;
    close();
    const sub = [...path, sheet.id];
    await api.navigateToSheet(sub);
    for (let i = 0; i < 40 && !samePath(api.getState().currentSheetPath, sub); i++) await new Promise((r) => setTimeout(r, 25));
    const last = lastPlaced();
    const items: CarriedItem[] = pins.map((p) => ({ kind: "label", scope: "hierarchical", text: p.name, shape: p.shape ?? "passive", spin: last.spin || "right", sizeNm: null, bold: last.bold, italic: last.italic, autoRotate: last.autoRotate }));
    dispatch({ type: "SET_DRAW_STATE", draw: null });
    dispatch({ type: "SET_ACTIVE_TOOL", tool: "sch_label_hier" });
    const c = api.getState().cursorNm;
    carryItem({ api }, "sch_label_hier", items[0]!, c ? [c.x, c.y] : [sheet.at[0], sheet.at[1]], items.slice(1));
    // `EndPlaceItem` / `CanPlaceMore` / `EndPlacement`, then `Show( true )`: the dialog again, on this sheet, once the hand is empty
    const again: SchToolDialog = { kind: "sync_pins", sheetIds: dialog.sheetIds, first: sheet.id, path: [...path] };
    const stop = subscribeSchCarry(() => {
      if (getSchCarry()) return;
      stop();
      dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: again });
    });
  };
  const status: Record<string, string> = { ok: "In sync", shape: "Shape differs from the label", pin_only: "No hierarchical label of this name in the sheet's file", label_only: "No pin on the sheet" };

  return (
    <SchDialogShell title="Synchronize Sheet Pins" width={620} onCancel={close} cancelLabel="Close">
      {sheets.length > 1 && (
        <div style={{ display: "flex", gap: 4, flexWrap: "wrap", marginBottom: 8 }}>
          {sheets.map((s, i) => (
            <button key={s.id} className={i === page ? "primary" : undefined} onClick={() => setPage(i)}>
              {s.name}
            </button>
          ))}
        </div>
      )}
      <div style={{ marginBottom: 6 }}>
        Sheet <b>{sheet.name}</b> ({sheet.file})
      </div>
      {!labels ? (
        <div className="panel-empty">Reading the sheet's hierarchical labels...</div>
      ) : rows.length === 0 ? (
        <div className="panel-empty">This sheet has no pins and its file no hierarchical labels.</div>
      ) : (
        <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }}>
          <thead>
            <tr style={{ textAlign: "left" }}>
              <th>Name</th>
              <th>Pin</th>
              <th>Label</th>
              <th>Status</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={`${r.kind}:${r.name}`}>
                <td>{r.name}</td>
                <td>{"pin" in r ? (r.pin.shape ?? "passive") : "-"}</td>
                <td>{"label" in r ? (r.label.shape ?? "passive") : "-"}</td>
                <td>{status[r.kind]}</td>
                <td>
                  {r.kind === "shape" && <button onClick={() => void send([{ op: "sch_edit", verb: "edit_sheet_pin", id: r.pin.id, shape: r.label.shape ?? "passive" } as Cmd])}>Use label's shape</button>}
                  {r.kind === "pin_only" && (
                    <>
                      <button onClick={() => void addLabels([r.pin])}>Add label</button> <button onClick={() => void send([{ op: "sch_edit", verb: "delete_sheet_pin", id: r.pin.id } as Cmd])}>Delete pin</button>
                    </>
                  )}
                  {r.kind === "label_only" && <button onClick={() => addPins([r.name])}>Add pin</button>}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {labels && (missing.length > 0 || unreferenced.length > 0) && (
        <div style={{ display: "flex", gap: 6, marginTop: 10 }}>
          {missing.length > 0 && <button onClick={() => addPins(missing.map((r) => r.name))}>Add all missing pins ({missing.length})</button>}
          {unreferenced.length > 0 && <button onClick={() => void addLabels(unreferenced.map((r) => (r as Extract<typeof r, { kind: "pin_only" }>).pin))}>Add labels ({unreferenced.length})</button>}
          {unreferenced.length > 0 && (
            <button onClick={() => void send(unreferenced.map((r): Cmd => ({ op: "sch_edit", verb: "delete_sheet_pin", id: (r as Extract<typeof r, { kind: "pin_only" }>).pin.id })))}>Delete unreferenced pins ({unreferenced.length})</button>
          )}
        </div>
      )}
    </SchDialogShell>
  );
}
