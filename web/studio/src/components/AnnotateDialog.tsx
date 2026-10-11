// Annotate Schematic -- `eeschema.EditorControl.annotate` (Ctrl+A): eeschema/dialogs/dialog_annotate.cpp (DIALOG_ANNOTATE) and
// SCH_EDIT_FRAME::AnnotateSymbols (eeschema/annotate.cpp) at 8303b2ad.
//
//   Scope      Entire schematic / Current sheet only / Selection, and "Recurse into subsheets"
//   Order      Sort symbols by X position (the default) / by Y position
//   Options    Keep existing annotations / Reset existing annotations, and "Regroup symbol units" (with a reset)
//   Numbering  Use first free number after [n] / First free after sheet number X 100 / X 1000
//   Annotation Messages: what each symbol was given, and the findings of the final check (CheckAnnotate)
//
// The dialog is modeless in KiCad: Annotate leaves it open with the messages. The numbering is the server's (crates/model/src/annotate.rs); the
// dialog asks it what it would do first (`POST /api/sch/annotate`, nothing written) so the message window shows it, then sends the `annotate` verb --
// one undo step -- when there is something to change. Not here: the "Clear Annotation" button -- this design names a part by its reference, so
// two symbols cannot both be `R?`; "Reset existing annotations" numbers again at once.
import { useEffect, useState } from "react";
import { fetchAnnotateReport } from "../api/hierarchyClient";
import type { AnnotateMessage } from "../api/hierarchyTypes";
import { annotateCmd, annotateRequest, rememberAnnotateSettings, rememberedAnnotateSettings, scopeOnOpen, type AnnotateSelection, type AnnotateSettings } from "../kicad-port/schAnnotate";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";

const severityColor: Record<AnnotateMessage["severity"], string> = { action: "var(--chrome-text)", warning: "#d4a017", error: "#e5484d" };

export function AnnotateDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.annotateDialogOpen;

  const selection: AnnotateSelection = {
    symbols: [...new Set([...state.selection].filter((id) => api.symbolById(id)))],
    sheets: [...state.selection].filter((id) => state.schematic?.sheets.some((s) => s.id === id)),
  };
  const hasSelection = selection.symbols.length > 0 || selection.sheets.length > 0;

  const [settings, setSettings] = useState<AnnotateSettings>(rememberedAnnotateSettings);
  const [messages, setMessages] = useState<AnnotateMessage[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    setSettings({ ...rememberedAnnotateSettings(), scope: scopeOnOpen(rememberedAnnotateSettings().scope, selection) });
    setMessages([]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_ANNOTATE_DIALOG_OPEN", open: false });
  const update = (patch: Partial<AnnotateSettings>) =>
    setSettings((s) => {
      const next = { ...s, ...patch };
      rememberAnnotateSettings(next);
      return next;
    });

  /** `OnAnnotateClick`: annotate, and report. */
  const annotate = async () => {
    setBusy(true);
    try {
      const reply = await fetchAnnotateReport(annotateRequest(settings, selection), state.currentSheetPath);
      if (!reply.ok) {
        setMessages([{ severity: "error", text: reply.message ?? "Annotation failed." }]);
        return;
      }
      let lines = reply.messages;
      if (reply.changed > 0) {
        const ok = await api.cmd(annotateCmd(settings, selection));
        if (!ok) lines = [...lines, { severity: "error", text: "The annotation was refused." }];
      }
      setMessages(lines);
    } finally {
      setBusy(false);
    }
  };

  const scopeRadio = (id: AnnotateSettings["scope"], label: string, enabled = true) => (
    <label style={{ display: "block", opacity: enabled ? 1 : 0.5 }}>
      <input type="radio" name="annotate-scope" checked={settings.scope === id} disabled={!enabled} onChange={() => update({ scope: id })} /> {label}
    </label>
  );

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog annotate-dialog" style={{ width: 520 }} onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Escape" && close()}>
        <div className="dialog-header">
          <span>Annotate Schematic</span>
        </div>
        <div className="dialog-body">
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
            <fieldset>
              <legend>Scope</legend>
              {scopeRadio("all", "Entire schematic")}
              {scopeRadio("current_sheet", "Current sheet only")}
              {scopeRadio("selection", `Selection${hasSelection ? ` (${selection.symbols.length} symbol${selection.symbols.length === 1 ? "" : "s"}${selection.sheets.length > 0 ? `, ${selection.sheets.length} sheet${selection.sheets.length === 1 ? "" : "s"}` : ""})` : ""}`, hasSelection)}
              <label style={{ display: "block", marginLeft: 18, opacity: settings.scope === "all" ? 0.5 : 1 }}>
                <input type="checkbox" checked={settings.recursive} disabled={settings.scope === "all"} onChange={(e) => update({ recursive: e.target.checked })} /> Recurse into subsheets
              </label>
            </fieldset>
            <fieldset>
              <legend>Order</legend>
              <label style={{ display: "block" }}>
                <input type="radio" name="annotate-order" checked={settings.sort === "x"} onChange={() => update({ sort: "x" })} /> Sort symbols by X position
              </label>
              <label style={{ display: "block" }}>
                <input type="radio" name="annotate-order" checked={settings.sort === "y"} onChange={() => update({ sort: "y" })} /> Sort symbols by Y position
              </label>
            </fieldset>
            <fieldset>
              <legend>Options</legend>
              <label style={{ display: "block" }}>
                <input type="radio" name="annotate-options" checked={!settings.reset} onChange={() => update({ reset: false })} /> Keep existing annotations
              </label>
              <label style={{ display: "block" }}>
                <input type="radio" name="annotate-options" checked={settings.reset} onChange={() => update({ reset: true })} /> Reset existing annotations
              </label>
              <label style={{ display: "block", marginLeft: 18, opacity: settings.reset ? 1 : 0.5 }}>
                <input type="checkbox" checked={settings.regroupUnits} disabled={!settings.reset} onChange={(e) => update({ regroupUnits: e.target.checked })} /> Regroup symbol units
              </label>
            </fieldset>
            <fieldset>
              <legend>Numbering</legend>
              <label style={{ display: "block" }}>
                <input type="radio" name="annotate-algo" checked={settings.algo === "first_free"} onChange={() => update({ algo: "first_free" })} /> Use first free number after:{" "}
                <input style={{ width: 56 }} value={settings.start} disabled={settings.algo !== "first_free"} onChange={(e) => update({ start: e.target.value })} />
              </label>
              <label style={{ display: "block" }}>
                <input type="radio" name="annotate-algo" checked={settings.algo === "sheet_x100"} onChange={() => update({ algo: "sheet_x100" })} /> First free after sheet number X 100
              </label>
              <label style={{ display: "block" }}>
                <input type="radio" name="annotate-algo" checked={settings.algo === "sheet_x1000"} onChange={() => update({ algo: "sheet_x1000" })} /> First free after sheet number X 1000
              </label>
            </fieldset>
          </div>
          <div style={{ marginTop: 10, fontSize: 11, color: "var(--chrome-text-dim)" }}>Annotation Messages:</div>
          <div
            data-testid="annotate-messages"
            style={{ height: 150, overflow: "auto", border: "1px solid var(--chrome-border, #444)", padding: 4, fontFamily: "var(--mono, monospace)", fontSize: 11 }}
          >
            {messages.map((m, i) => (
              <div key={i} style={{ color: severityColor[m.severity] }}>
                {m.text}
              </div>
            ))}
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Close</button>
          <button className="primary" disabled={busy} onClick={() => void annotate()}>
            Annotate
          </button>
        </div>
      </div>
    </div>
  );
}
