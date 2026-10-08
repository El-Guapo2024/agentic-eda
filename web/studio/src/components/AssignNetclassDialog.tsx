// Assign Netclass... -- port of common/dialogs/dialog_assign_netclass.cpp (DIALOG_ASSIGN_NETCLASS), opened by
// `pcbnew.EditorControl.assignNetclass` and `eeschema.InteractiveEdit.assignNetclass` (actions/assignNetclass.ts) for the nets of the
// selected items. The pattern starts as KiCad proposes it for those nets (`GetNetclassPatternForSet`), the class as the first one besides
// Default, and the box lists the nets the pattern matches as it is typed. OK adds the assignment to the Net Classes page
// (`set_net_classes`, one undo step on the board's side), exactly what Board Setup > Net Classes would store; an empty pattern assigns nothing.
//
// Not ported: the previewer that selects every item on the matching nets on the canvas while the pattern is typed.
import { useState } from "react";
import { closeSweepDialog } from "../actions/pcbSweepDialogs";
import { netClassSettingsOf, netsMatching, proposePattern, withAssignment } from "../kicad-port/boardSetupRules";
import { useStudioApi, useStudioState } from "../state/store";
import { DialogShell } from "./pcbDialogKit";
import "../styles/boardSetup.css";

export function AssignNetclassDialog({ nets, candidates }: { nets: readonly string[]; candidates: readonly string[] }) {
  const state = useStudioState();
  const api = useStudioApi();
  const rules = state.board?.board_rules;
  const settings = rules ? netClassSettingsOf(rules) : null;
  const classNames = settings ? [settings.default.name, ...settings.classes.map((c) => c.name)] : [];
  const [pattern, setPattern] = useState(() => proposePattern(nets));
  // `m_netclassCtrl->SetSelection( 1 )`: the first class besides Default when there is one.
  const [netclass, setNetclass] = useState(() => classNames[1] ?? classNames[0] ?? "Default");
  const [busy, setBusy] = useState(false);
  const matching = pattern.trim() === "" ? [] : netsMatching(pattern, candidates);

  const onOk = async () => {
    if (!settings || pattern.trim() === "") return closeSweepDialog();
    setBusy(true);
    try {
      if (await api.cmd({ op: "set_net_classes", settings: withAssignment(settings, pattern, netclass) })) closeSweepDialog();
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogShell title="Assign Netclass" width={460} onCancel={closeSweepDialog} onOk={() => void onOk()} okDisabled={busy || !settings}>
      <label style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6, fontSize: 12 }}>
        <span style={{ minWidth: 64 }}>Pattern:</span>
        <input
          autoFocus
          aria-label="Pattern"
          style={{ flex: 1 }}
          value={pattern}
          spellCheck={false}
          onChange={(e) => setPattern(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void onOk();
          }}
        />
      </label>
      <label style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6, fontSize: 12 }}>
        <span style={{ minWidth: 64 }}>Net class:</span>
        <select aria-label="Net class" style={{ flex: 1 }} value={netclass} onChange={(e) => setNetclass(e.target.value)}>
          {classNames.map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </select>
      </label>
      <div className="bs-matches" style={{ maxHeight: 150 }} aria-label="Currently matching nets">
        <b>Currently matching nets:</b>
        {matching.map((n) => (
          <div key={n}>{n}</div>
        ))}
      </div>
      <p className="bs-note">
        Note: complete netclass assignments can be edited in Board Setup &gt; Design Rules &gt; Net Classes.
        {classNames.length < 2 ? " There is no net class besides Default yet: add one there first." : ""}
      </p>
    </DialogShell>
  );
}
