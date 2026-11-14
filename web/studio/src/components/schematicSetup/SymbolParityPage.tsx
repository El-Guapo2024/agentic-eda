// Schematic Setup > Electrical Rules > Compare Symbol with Library -- port of KiCad 25ec2335 eeschema/dialogs/panel_setup_symbol_parity.cpp
// (`PANEL_SETUP_SYMBOL_PARITY`: `transferSettingsToWindow`, `TransferDataFromWindow`, `ImportSettingsFrom`): the twelve boxes of `SYMBOL_PARITY_SETTINGS` in
// the panel's three groups (Fields, Pins, Attributes). They say which differences between a placed symbol and its library symbol count, for ERC's
// `lib_symbol_mismatch` (kicad-cli reads the derived project's `schematic.compare_symbols`) and for Inspect > Compare Symbol with Library
// (`SchematicSettings::SymbolCompareFlags`, kicad-port/symbolDiff.ts). Apply is one undoable `set_symbol_parity` (UI-LEFT-3 B3-9).
import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchSchSetup } from "../../api/client";
import type { SchSetupReply, SymbolParitySettings } from "../../api/types";
import { DEFAULT_SYMBOL_PARITY, SYMBOL_PARITY_GROUPS } from "../../kicad-port/schSetup";
import { useStudioState } from "../../state/store";
import { Check, PageFrame, usePageApply, usePageDraft, type PageProps, useSchSetupImport } from "../boardSetup/fields";

function draftOf(r: SchSetupReply): SymbolParitySettings {
  return { ...DEFAULT_SYMBOL_PARITY, ...(r.symbol_parity ?? {}) };
}

export function SymbolParityPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const [info, setInfo] = useState<SchSetupReply | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const load = useCallback(async () => {
    try {
      const r = await fetchSchSetup();
      if (r.ok) {
        setInfo(r);
        setProblem(null);
      } else setProblem(r.message ?? "The backend could not read the schematic's symbol comparison settings.");
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load, state.version]);

  const source = useMemo(() => (info ? draftOf(info) : undefined), [info]);
  const page = usePageDraft<SymbolParitySettings>(source);
  // "Import Settings from Another Project..." > "Compare symbol with library settings" (`ImportSettingsFrom( schematicSettings.m_SymbolParity )`)
  useSchSetupImport("symbol_parity", (s) => draftOf(s.setup), page.importDraft);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const d = page.draft;

  const onApply = () => {
    if (!d || !page.dirty) return;
    void apply.run({ op: "set_symbol_parity", settings: d }, () => {
      page.markClean();
      void load();
    });
  };

  return (
    <PageFrame title="Compare Symbol with Library" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? problem} bad={apply.message?.bad || problem !== null} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!d ? (
        <p className="bs-note">{problem ?? "Reading the schematic's symbol comparison settings…"}</p>
      ) : (
        SYMBOL_PARITY_GROUPS.map((g) => (
          <div key={g.title}>
            <h4 className="bs-subhead">{g.title}</h4>
            {g.boxes.map((b) => (
              <Check key={b.key} checked={d[b.key]} onChange={(v) => page.edit((x) => ({ ...x, [b.key]: v }))}>
                {b.label}
              </Check>
            ))}
          </div>
        ))
      )}
    </PageFrame>
  );
}
