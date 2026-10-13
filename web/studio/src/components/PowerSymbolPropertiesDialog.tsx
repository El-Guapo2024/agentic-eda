// Symbol Properties of a power symbol (`E`, a double-click; `SCH_EDIT_TOOL::Properties` opens `DIALOG_SYMBOL_PROPERTIES` for it, as it does for every `SCH_SYMBOL_T`, with the Spice
// model button hidden because the library symbol is a power one: eeschema/dialogs/dialog_symbol_properties.cpp at 8303b2ad). A power symbol's Reference is `#PWR..` and not shown, its Value
// is the net it asserts. The dialog has what the drawing model keeps of a power symbol: the Reference (as it is), the Value (renaming the net of this symbol) and whether each is shown on
// the sheet, and the library link. OK is one command (kicad-port/schProperties.ts `powerEditCmd`), so one undo step, and nothing is sent when nothing changed.
//
// Not here, because a power symbol has no place for them in the IR: a footprint and a datasheet, the exclusion flags, the pin table and embedded files.
import { useState } from "react";
import { powerEditCmd } from "../kicad-port/schProperties";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { SchDialogShell } from "./SchDialogShell";

export function PowerSymbolPropertiesDialog({ id }: { id: string }) {
  const power = useStudioState().schematic?.power_symbols.find((p) => p.id === id);
  return power ? <PowerForm key={id} power={power} /> : null;
}

function PowerForm({ power }: { power: { id: string; lib_id: string; net: string; fields?: ReadonlyArray<{ name: string; visible: boolean }> } }) {
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  const shownOf = (name: string) => power.fields?.find((f) => f.name === name)?.visible;
  const [net, setNet] = useState(power.net);
  const [showReference, setShowReference] = useState(shownOf("Reference") ?? false);
  const [showValue, setShowValue] = useState(shownOf("Value") ?? true);
  // a Value cannot be emptied (`FIELD_VALIDATOR`): a power symbol needs the net it asserts
  const canOk = net.trim().length > 0;
  const submit = () => {
    if (!canOk) return;
    const cmd = powerEditCmd(power, { net, showReference, showValue });
    if (!cmd) return close();
    void api.cmd(cmd).then((ok) => ok && close());
  };
  return (
    <SchDialogShell title="Symbol Properties" width={460} onCancel={close} onOk={submit} okLabel="OK" canOk={canOk}>
      <div className="kv-grid" style={{ gridTemplateColumns: "100px 1fr auto" }}>
        <span>Reference</span>
        <input aria-label="Reference" value={power.id} readOnly disabled title="A power symbol's reference is #PWR with a number" />
        {shownOf("Reference") === undefined ? (
          <span />
        ) : (
          <label style={{ whiteSpace: "nowrap" }} title="Show the Reference field on the sheet">
            <input type="checkbox" aria-label="Show Reference" checked={showReference} onChange={(e) => setShowReference(e.target.checked)} /> Show
          </label>
        )}
        <span>Value</span>
        <input aria-label="Value" autoFocus value={net} onChange={(e) => setNet(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} title="The net this power symbol asserts" />
        {shownOf("Value") === undefined ? (
          <span />
        ) : (
          <label style={{ whiteSpace: "nowrap" }} title="Show the Value field on the sheet">
            <input type="checkbox" aria-label="Show Value" checked={showValue} onChange={(e) => setShowValue(e.target.checked)} /> Show
          </label>
        )}
        <span>Library link</span>
        <span style={{ opacity: 0.7 }}>{power.lib_id}</span>
        <span />
      </div>
    </SchDialogShell>
  );
}
