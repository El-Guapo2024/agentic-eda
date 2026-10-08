// Compare Symbol with Library (`eeschema.InspectionTool.diffSymbol`, `SCH_INSPECTION_TOOL::DiffSymbol`): the report of how the symbol the schematic
// draws differs from its library entry -- `kicad-port/symbolDiff.ts`. KiCad's "Visual" page (the two drawn side by side) is not ported.
import { useEffect, useState } from "react";
import { useStudioState } from "../../state/store";
import { fetchAnySymbol } from "../../api/libraryClient";
import { diffSymbols } from "../../kicad-port/symbolDiff";
import { SchDialogFrame } from "./SchDialogFrame";

type Report = { kind: "missing"; item: string } | { kind: "diff"; lines: string[] } | null;

export function SymbolDiffDialog({ reference, onClose }: { reference: string; onClose: () => void }) {
  const state = useStudioState();
  const sym = state.schematic?.symbols.find((s) => s.id === reference);
  const libId = sym?.lib_id ?? "";
  const [lib, item] = libId.includes(":") ? [libId.slice(0, libId.indexOf(":")), libId.slice(libId.indexOf(":") + 1)] : ["", libId];
  const [report, setReport] = useState<Report>(null);

  useEffect(() => {
    let alive = true;
    const drawn = libId ? state.schematic?.lib_symbols[libId] : undefined;
    if (!drawn) {
      setReport({ kind: "missing", item });
      return;
    }
    fetchAnySymbol(libId).then(
      ({ symbol }) => alive && setReport({ kind: "diff", lines: diffSymbols({ ...drawn, power: (drawn as { power?: boolean }).power ?? false }, symbol) }),
      () => alive && setReport({ kind: "missing", item })
    );
    return () => {
      alive = false;
    };
    // The report is for the symbol as it was when the dialog opened.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [libId]);

  return (
    <SchDialogFrame title="Compare Symbol with Library" width={620} onClose={onClose}>
      <b>Schematic vs library diff for:</b>
      <ul style={{ marginTop: 4 }}>
        <li>Symbol {reference}</li>
        <li>Library: {lib || "(none)"}</li>
        <li>Library item: {item}</li>
      </ul>
      {report === null && <p>Comparing…</p>}
      {report?.kind === "missing" && <p>The library no longer contains the item {report.item}.</p>}
      {report?.kind === "diff" && (report.lines.length === 0 ? <p>No relevant differences detected.</p> : <ul>{report.lines.map((l, i) => <li key={i} style={{ marginBottom: 4 }}>{l}</li>)}</ul>)}
    </SchDialogFrame>
  );
}
