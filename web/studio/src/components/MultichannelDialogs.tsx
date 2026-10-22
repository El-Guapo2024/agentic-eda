// The multichannel tool's two dialogs (GAPS.md items 24 and 32, package P24): Generate Placement Rule Areas (`DIALOG_MULTICHANNEL_GENERATE_RULE_AREAS`,
// pcbnew/dialogs/dialog_multichannel_generate_rule_areas.cpp) and Repeat Layout (`DIALOG_MULTICHANNEL_REPEAT_LAYOUT`, dialog_multichannel_repeat_layout.cpp), at 8303b2ad.
// What each does is the server's (crates/ops/src/multichannel*.rs); the arithmetic of the choices is kicad-port/multichannel.ts. Opened by actions/pcbMultichannelActions.ts.
import { useState } from "react";
import type { RepeatPlan } from "../api/multichannelClient";
import { useStudioApi, useStudioDispatch } from "../state/store";
import {
  anchorChoices,
  chooseSourceFlags,
  copiedMessage,
  GENERATE_TABS,
  generateCommand,
  groupItemsAvailable,
  initialGenerateTab,
  mismatchSummary,
  repeatCommand,
  statusOf,
  targetRows,
  type GenerateRow,
  type RepeatOptions,
  type TargetRow,
} from "../kicad-port/multichannel";
import { closeGenerateDialog, closeRepeatDialog, lastGenerateFlags, lastRepeatOptions, rememberGenerateFlags, rememberRepeatOptions, useMultichannelDialogs } from "../state/multichannelDialogs";
import { DialogShell } from "./pcbDialogKit";

export function MultichannelDialogs() {
  const { generate, repeat } = useMultichannelDialogs();
  return (
    <>
      {generate && <GenerateDialog initial={generate.rows} />}
      {repeat && <RepeatDialog plan={repeat.plan} />}
    </>
  );
}

const GRID: React.CSSProperties = { width: "100%", borderCollapse: "collapse", fontSize: 12 };
const CELL: React.CSSProperties = { border: "1px solid var(--chrome-border, #444)", padding: "2px 6px", textAlign: "left" };

// ------------------------------------------------------------------------------------------------------------------------ Generate Placement Rule Areas

function GenerateDialog({ initial }: { initial: GenerateRow[] }) {
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const [rows, setRows] = useState<GenerateRow[]>(initial);
  const [tab, setTab] = useState<GenerateRow["kind"]>(() => initialGenerateTab(initial));
  const [flags, setFlags] = useState(() => lastGenerateFlags());
  const shown = rows.filter((r) => r.kind === tab);
  const toggle = (row: GenerateRow, on: boolean) => setRows((all) => all.map((r) => (r === row ? { ...r, generate: on } : r)));
  const submit = async () => {
    const cmd = generateCommand(rows, flags.replace, flags.group, tab);
    rememberGenerateFlags(flags);
    closeGenerateDialog();
    // `TransferDataFromWindow` with no row ticked leaves nothing to make
    if (!cmd) return;
    const ok = await api.cmd(cmd);
    if (ok) dispatch({ type: "TOAST", message: "Placement rule areas generated.", kind: "info" });
  };
  return (
    <DialogShell title="Generate Placement Rule Areas" width={640} onCancel={closeGenerateDialog} onOk={() => void submit()}>
      <div data-testid="mc-generate-dialog">
        <div role="tablist" style={{ display: "flex", gap: 4, marginBottom: 6 }}>
          {GENERATE_TABS.map((t) => (
            <button key={t.kind} role="tab" aria-selected={tab === t.kind} data-testid={`mc-generate-tab-${t.kind}`} className={tab === t.kind ? "primary" : undefined} onClick={() => setTab(t.kind)}>
              {t.label}
            </button>
          ))}
        </div>
        <div style={{ maxHeight: 280, overflowY: "auto", minHeight: 140 }}>
          <table style={GRID} data-testid={`mc-generate-grid-${tab}`}>
            <thead>
              <tr>
                <th style={CELL}>Generate</th>
                {tab === "sheet" && (
                  <>
                    <th style={CELL}>Sheet Path</th>
                    <th style={CELL}>Sheet Name</th>
                  </>
                )}
                {tab === "component_class" && <th style={CELL}>Component Class</th>}
                {tab === "group" && <th style={CELL}>Name</th>}
              </tr>
            </thead>
            <tbody>
              {shown.length === 0 && (
                <tr>
                  <td style={CELL} colSpan={3}>
                    (none)
                  </td>
                </tr>
              )}
              {shown.map((r) => (
                <tr key={`${r.kind}:${r.key}`} data-testid={`mc-generate-row-${r.kind}-${r.key}`} title={`${r.components.length} footprint${r.components.length === 1 ? "" : "s"}: ${r.components.join(", ")}`}>
                  <td style={CELL}>
                    <input type="checkbox" aria-label={`Generate ${r.key}`} data-testid={`mc-generate-check-${r.kind}-${r.key}`} checked={r.generate} onChange={(e) => toggle(r, e.target.checked)} />
                  </td>
                  <td style={CELL}>{r.key}</td>
                  {tab === "sheet" && <td style={CELL}>{r.file}</td>}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <label style={{ display: "block", marginTop: 10 }}>
          <input type="checkbox" data-testid="mc-generate-replace" checked={flags.replace} onChange={(e) => setFlags((f) => chooseSourceFlags(f, { replace: e.target.checked }))} /> Replace existing placement rule areas
        </label>
        <label style={{ display: "block", marginTop: 4, opacity: groupItemsAvailable(tab) ? 1 : 0.5 }}>
          <input type="checkbox" data-testid="mc-generate-group" disabled={!groupItemsAvailable(tab)} checked={groupItemsAvailable(tab) && flags.group} onChange={(e) => setFlags((f) => chooseSourceFlags(f, { group: e.target.checked }))} /> Group footprints with their placement rule areas
        </label>
      </div>
    </DialogShell>
  );
}

// ------------------------------------------------------------------------------------------------------------------------------------------ Repeat Layout

function RepeatDialog({ plan }: { plan: RepeatPlan }) {
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const [rows, setRows] = useState<TargetRow[]>(() => targetRows(plan.targets));
  const [options, setOptions] = useState<RepeatOptions>(() => lastRepeatOptions());
  const [details, setDetails] = useState<TargetRow | null>(null);
  const set = <K extends keyof RepeatOptions>(key: K, value: RepeatOptions[K]) => setOptions((o) => ({ ...o, [key]: value }));
  const toggle = (id: string, on: boolean) => setRows((all) => all.map((r) => (r.id === id ? { ...r, copy: on } : r)));
  const submit = async () => {
    const cmd = repeatCommand(plan.id, rows, options);
    rememberRepeatOptions(options);
    closeRepeatDialog();
    if (!cmd) return;
    const n = cmd.op === "multichannel" && cmd.verb === "repeat_layout" ? cmd.targets.length : 0;
    const ok = await api.cmd(cmd);
    if (ok) dispatch({ type: "TOAST", message: copiedMessage(n), kind: "info" });
  };
  const anyTarget = rows.some((r) => r.copy && r.ok);
  return (
    <>
      <DialogShell title="Repeat Layout" width={680} onCancel={closeRepeatDialog} onOk={() => void submit()} okDisabled={!anyTarget}>
        <div data-testid="mc-repeat-dialog">
          <div className="kv-grid" style={{ gridTemplateColumns: "150px 1fr" }}>
            <span>Reference rule area:</span>
            <span data-testid="mc-repeat-reference">{plan.name || plan.id}</span>
            <span style={{ paddingLeft: 15 }}>Anchor footprint:</span>
            <select
              data-testid="mc-repeat-anchor"
              value={options.anchor ?? ""}
              title={"Optional, use for precise and/or rotated placement.\nSelect reference rule area footprint, place corresponding\ntarget rule area footprint(s)."}
              onChange={(e) => set("anchor", e.target.value || null)}
            >
              {anchorChoices(plan.components).map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          </div>
          <p style={{ margin: "10px 0 4px" }}>Target areas:</p>
          <div style={{ maxHeight: 220, overflowY: "auto", minHeight: 80 }}>
            <table style={GRID} data-testid="mc-repeat-grid">
              <thead>
                <tr>
                  <th style={CELL}>Copy</th>
                  <th style={CELL}>Target Rule Area</th>
                  <th style={CELL}>Status</th>
                  <th style={CELL}>Details</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.id} data-testid={`mc-repeat-target-${r.id}`}>
                    <td style={CELL}>
                      <input type="checkbox" aria-label={`Copy to ${r.name || r.id}`} data-testid={`mc-repeat-copy-${r.id}`} checked={r.copy} onChange={(e) => toggle(r.id, e.target.checked)} />
                    </td>
                    <td style={CELL}>{r.name || r.id}</td>
                    <td style={CELL} data-testid={`mc-repeat-status-${r.id}`}>
                      {statusOf(r)}
                    </td>
                    <td style={CELL}>
                      {!r.ok && r.reasons.length > 0 && (
                        <button aria-label={`Details of ${r.name || r.id}`} data-testid={`mc-repeat-details-${r.id}`} onClick={() => setDetails(r)}>
                          ?
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div style={{ marginTop: 10 }}>
            <label style={{ display: "block", margin: "4px 0" }}>
              <input type="checkbox" data-testid="mc-repeat-opt-copy_placement" checked={options.copy_placement} onChange={(e) => set("copy_placement", e.target.checked)} /> Copy footprint placement
            </label>
            <label style={{ display: "block", margin: "4px 0" }}>
              <input type="checkbox" data-testid="mc-repeat-opt-copy_routing" checked={options.copy_routing} onChange={(e) => set("copy_routing", e.target.checked)} /> Copy routing
            </label>
            <label style={{ display: "block", margin: "4px 0 4px 25px" }} title="Can be useful if unrelated tracks pass through the area">
              <input type="checkbox" data-testid="mc-repeat-opt-connected_routing_only" checked={options.connected_routing_only} onChange={(e) => set("connected_routing_only", e.target.checked)} /> Restrict to routing connected within the area
            </label>
            <label style={{ display: "block", margin: "4px 0" }} title="Copy text, shapes, zones, and other items inside the source rule area">
              <input type="checkbox" data-testid="mc-repeat-opt-copy_other_items" checked={options.copy_other_items} onChange={(e) => set("copy_other_items", e.target.checked)} /> Copy other items
            </label>
            <label style={{ display: "block", margin: "12px 0 4px" }}>
              <input type="checkbox" data-testid="mc-repeat-opt-group_items" checked={options.group_items} onChange={(e) => set("group_items", e.target.checked)} /> Group items with their target rule areas
            </label>
            <label style={{ display: "block", margin: "4px 0" }} title="Copy from reference area + delete / update in target area if included">
              <input type="checkbox" data-testid="mc-repeat-opt-include_locked_items" checked={options.include_locked_items} onChange={(e) => set("include_locked_items", e.target.checked)} /> Include locked items
            </label>
          </div>
        </div>
      </DialogShell>
      {details && (
        <div className="dialog-backdrop" style={{ zIndex: 1100 }} onClick={() => setDetails(null)}>
          <div className="dialog" style={{ width: 560 }} data-testid="mc-repeat-mismatch" onClick={(e) => e.stopPropagation()}>
            <div className="dialog-header">
              <span>Topology mismatch</span>
            </div>
            <div className="dialog-body">
              <p style={{ marginTop: 0 }}>{mismatchSummary(details)}</p>
              <pre style={{ maxHeight: 260, overflow: "auto", fontSize: 12, whiteSpace: "pre-wrap" }} data-testid="mc-repeat-mismatch-reasons">
                {details.reasons.join("\n")}
              </pre>
            </div>
            <div className="dialog-footer">
              <button className="primary" data-testid="mc-repeat-mismatch-ok" onClick={() => setDetails(null)}>
                OK
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
