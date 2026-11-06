// File > Import > Netlist... (`DIALOG_IMPORT_NETLIST`, pcbnew/dialogs/dialog_import_netlist.cpp): the netlist file, the Link Method, the Options, the message
// panel and its three buttons. Load and Test Netlist (and every change of a choice, `OnMatchChanged` / `OnOptionChanged`) shows the dry run; Update PCB applies it as
// one undo step and shows what was done; Close hands the footprints the update added to the Move tool (`m_runDragCommand`), spread as `SpreadFootprints` spreads
// them. The flow is kicad-port/importNetlist.ts's; the work is the server's (crates/ops/src/netlist_update.rs).
import { useCallback, useRef, useState } from "react";
import { closeSweepDialog } from "../actions/pcbSweepDialogs";
import { postCarry, postCmd } from "../api/client";
import { testNetlist } from "../api/importClient";
import { pickTextFile } from "../api/libraryClient";
import type { CmdReply } from "../api/types";
import {
  BUTTON_CLOSE,
  BUTTON_TEST,
  BUTTON_UPDATE,
  DEFAULT_NETLIST_OPTIONS,
  DIALOG_TITLE,
  FILE_LABEL,
  importNetlistCmd,
  LABEL_APPLIED,
  LABEL_TO_APPLY,
  LINK_CHOICES,
  LINK_METHOD_LABEL,
  LINK_TOOLTIP,
  NETLIST_ACCEPT,
  NO_SUCH_FILE,
  NO_VALID_FILE,
  OPTION_LABELS,
  OPTIONS_LABEL,
  SHOW_ALL,
  shownLines,
  type NetlistFile,
  type NetlistOptions,
  type NetlistReport,
  type NetlistReportLine,
  type ShownSeverities,
  updateOutcome,
} from "../kicad-port/importNetlist";
import { useEscape } from "./pcbDialogKit";

export interface ImportNetlistHost {
  /** The Strict switch. */
  strict: () => boolean;
  /** The board read again after the update. */
  refresh: () => Promise<void>;
  /** The added footprints handed to the Move tool (their refs and paths); nothing to carry is `[]`. */
  carry: (refs: string[], paths: string[]) => void;
  /** Says something on the canvas (a refused drop, ...). */
  toast: (message: string, kind: "error" | "info") => void;
}

const SHOW_LABELS: ReadonlyArray<readonly [keyof ShownSeverities, string]> = [
  ["error", "Errors"],
  ["warning", "Warnings"],
  ["action", "Actions"],
  ["info", "Infos"],
];

const COLOR: Record<NetlistReportLine["severity"], string> = { info: "var(--chrome-text-dim)", action: "var(--chrome-text)", warning: "#d4a017", error: "#e5484d" };

/** `given` is a file the action was run with (`window.__eda.run(id, { name, text })`, a test's way in; KiCad's dialog starts with the last path). */
export function ImportNetlistDialog({ host, given }: { host: ImportNetlistHost; given?: NetlistFile | null }) {
  const [file, setFile] = useState<NetlistFile | null>(given ?? null);
  const [options, setOptions] = useState<NetlistOptions>(DEFAULT_NETLIST_OPTIONS);
  const [lines, setLines] = useState<NetlistReportLine[]>([]);
  const [label, setLabel] = useState(LABEL_TO_APPLY);
  const [show, setShow] = useState<ShownSeverities>(SHOW_ALL);
  const [busy, setBusy] = useState(false);
  const [alert, setAlert] = useState<string | null>(null);
  // the footprints the last Update PCB added, and whether a step is held open for them (`carry: "begin"`)
  const pending = useRef<{ refs: string[]; paths: string[] } | null>(null);

  /** `loadNetlist( true )`: the dry run, the message panel cleared and filled. */
  const test = useCallback(
    async (f: NetlistFile | null, o: NetlistOptions) => {
      if (!f) return;
      setBusy(true);
      try {
        const r: NetlistReport = await testNetlist({ text: f.text, name: f.name, options: o });
        // `ReadNetlistFromFile` refuses a file in a message box (`DisplayErrorMessage`) and the panel keeps the head lines
        if (r.error) setAlert(r.error);
        setLines(r.lines ?? (r.message ? [{ severity: "error", text: r.message }] : []));
      } catch (e) {
        setLines([{ severity: "error", text: e instanceof Error ? e.message : String(e) }]);
      } finally {
        setBusy(false);
      }
    },
    []
  );

  const browse = async () => {
    const picked = await pickTextFile(NETLIST_ACCEPT);
    // `onFilenameChanged( false )`: the name is taken, the netlist is not read yet
    if (picked) setFile(picked);
  };

  const onLoadAndTest = () => {
    // `onFilenameChanged( true )`
    if (!file) {
      setLines([{ severity: "error", text: NO_SUCH_FILE }]);
      return;
    }
    // (the label stays as Update PCB left it: `onImportNetlist` does not set it)
    void test(file, options);
  };

  const change = (o: NetlistOptions) => {
    setOptions(o);
    // `OnMatchChanged` / `OnOptionChanged`: `loadNetlist( true )` when there is a file
    if (file) void test(file, o);
  };

  /** A step held open by an earlier Update PCB is ended first, judged as a drop that changes nothing (crates/cli/src/carry.rs). */
  const settlePending = async (): Promise<CmdReply | null> => {
    if (!pending.current) return null;
    pending.current = null;
    return postCarry("commit", host.strict());
  };

  const onUpdate = async () => {
    // `onUpdatePCB`: a message box when there is no file
    if (!file) {
      setAlert(NO_VALID_FILE);
      return;
    }
    setBusy(true);
    try {
      const earlier = await settlePending();
      if (earlier && !earlier.ok) host.toast(earlier.message, "error");
      setLabel(LABEL_APPLIED);
      // the step is held open (`carry: "begin"`): the footprints it adds are carried to where they are dropped, and the gates judge the import and the drop together
      const reply = (await postCmd(importNetlistCmd(file, options), host.strict(), "begin")) as CmdReply & { report?: NetlistReport };
      const outcome = updateOutcome(reply, file, options.by_timestamp);
      if (outcome.alert) setAlert(outcome.alert);
      let lines = outcome.lines;
      if (outcome.commitNow) {
        // nothing to carry: the step ends now, judged; a refusal takes the import back
        const done = await postCarry("commit", host.strict());
        if (!done.ok) lines = [...lines, { severity: "error", text: done.message }];
      }
      pending.current = outcome.carry;
      setLines(lines);
      await host.refresh();
    } finally {
      setBusy(false);
    }
  };

  const onClose = () => {
    closeSweepDialog();
    // `~DIALOG_IMPORT_NETLIST`: with footprints added, the Move tool picks them up
    const p = pending.current;
    pending.current = null;
    if (p) host.carry(p.refs, p.paths);
  };
  useEscape(onClose);

  const allShown = SHOW_LABELS.every(([k]) => show[k]);
  const shown = shownLines(lines, show);
  return (
    <div className="dialog-backdrop">
      <div className="dialog" style={{ width: 640 }} onClick={(e) => e.stopPropagation()} data-testid="import-netlist-dialog">
        <div className="dialog-header">
          <span>{DIALOG_TITLE}</span>
        </div>
        <div className="dialog-body">
          <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 8 }}>
            <label htmlFor="import-netlist-file">{FILE_LABEL}</label>
            <input id="import-netlist-file" data-testid="import-netlist-file" style={{ flex: 1 }} readOnly value={file?.name ?? ""} />
            <button data-testid="import-netlist-browse" title={DIALOG_TITLE} onClick={() => void browse()}>
              …
            </button>
          </div>
          <div style={{ display: "flex", gap: 10 }}>
            <fieldset style={{ flex: 1 }} title={LINK_TOOLTIP}>
              <legend>{LINK_METHOD_LABEL}</legend>
              {LINK_CHOICES.map((text, i) => (
                <label key={text} style={{ display: "block" }}>
                  <input type="radio" name="import-netlist-link" data-testid={`import-netlist-link-${i}`} checked={options.by_timestamp === (i === 0)} onChange={() => change({ ...options, by_timestamp: i === 0 })} /> {text}
                </label>
              ))}
            </fieldset>
            <fieldset style={{ flex: 1 }}>
              <legend>{OPTIONS_LABEL}</legend>
              {OPTION_LABELS.map(([key, text]) => (
                <label key={key} style={{ display: "block" }}>
                  <input type="checkbox" data-testid={`import-netlist-${key}`} checked={options[key]} onChange={(e) => change({ ...options, [key]: e.target.checked })} /> {text}
                </label>
              ))}
            </fieldset>
          </div>
          <fieldset style={{ marginTop: 8 }}>
            <legend data-testid="import-netlist-label">{label}</legend>
            <div data-testid="import-netlist-messages" style={{ height: 230, overflow: "auto", border: "1px solid var(--chrome-border, #444)", padding: 4, fontFamily: "var(--mono, monospace)", fontSize: 11 }}>
              {shown.map((l, i) => (
                <div key={i} data-severity={l.severity} style={{ color: COLOR[l.severity], minHeight: "1em" }}>
                  {l.text}
                </div>
              ))}
            </div>
            <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 4, fontSize: 11 }}>
              <span>Show:</span>
              <label>
                <input type="checkbox" checked={allShown} onChange={(e) => setShow(e.target.checked ? SHOW_ALL : { error: false, warning: false, action: false, info: false })} /> All
              </label>
              {SHOW_LABELS.map(([k, text]) => (
                <label key={k}>
                  <input type="checkbox" data-testid={`import-netlist-show-${k}`} checked={show[k]} onChange={(e) => setShow({ ...show, [k]: e.target.checked })} /> {text}
                </label>
              ))}
            </div>
          </fieldset>
          {alert && (
            <div className="dialog-alert" data-testid="import-netlist-alert" style={{ marginTop: 6, color: "#e5484d", whiteSpace: "pre-wrap" }}>
              {alert}{" "}
              <button onClick={() => setAlert(null)}>OK</button>
            </div>
          )}
        </div>
        <div className="dialog-footer">
          <button data-testid="import-netlist-test" disabled={busy} onClick={onLoadAndTest}>
            {BUTTON_TEST}
          </button>
          <button className="primary" data-testid="import-netlist-update" disabled={busy} onClick={() => void onUpdate()}>
            {BUTTON_UPDATE}
          </button>
          <button data-testid="import-netlist-close" onClick={onClose}>
            {BUTTON_CLOSE}
          </button>
        </div>
      </div>
    </div>
  );
}
