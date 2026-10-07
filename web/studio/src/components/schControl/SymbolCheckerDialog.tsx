// Symbol Checker (`eeschema.InspectionTool.checkSymbol`, `SCH_INSPECTION_TOOL::CheckSymbol`): the warnings for the symbol open in the Symbol Editor, or
// "No symbol issues found." -- `kicad-port/symbolChecker.ts` is the check.
import { useSymState } from "../../state/symbolEditorStore";
import { useStudioState } from "../../state/store";
import { formatLength } from "../../state/units";
import { checkLibSymbol } from "../../kicad-port/symbolChecker";
import { SchDialogFrame } from "./SchDialogFrame";

export function SymbolCheckerDialog({ onClose }: { onClose: () => void }) {
  const sym = useSymState();
  const studio = useStudioState();
  const symbol = sym.symbol;
  if (!symbol) return null;
  // The editor's grid in internal units (nanometres): a pin must sit on at least a 25 mil grid.
  const gridIu = Math.round(sym.gridUm * 1000);
  const messages = checkLibSymbol(symbol, gridIu, (mm) => formatLength(mm * 1000, studio.units));
  return (
    <SchDialogFrame title={messages.length === 0 ? "Symbol Checker" : "Symbol Warnings"} width={560} onClose={onClose}>
      {messages.length === 0 ? (
        <p>No symbol issues found.</p>
      ) : (
        messages.map((m, i) => (
          <p key={i} style={{ margin: "0 0 12px", whiteSpace: "pre-line" }}>
            {m.map((part, j) => (part.bold ? <b key={j}>{part.text}</b> : <span key={j}>{part.text}</span>))}
          </p>
        ))
      )}
    </SchDialogFrame>
  );
}
