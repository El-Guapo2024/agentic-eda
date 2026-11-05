// Schematic Setup > General > BOM Presets -- port of eeschema/dialogs/panel_bom_presets.cpp: the project's saved presets, "Bill of Materials Presets"
// (`SCHEMATIC_SETTINGS::m_BomPresets`, the project's `bom_presets`) and "Bill of Materials Formatting Presets" (`m_BomFmtPresets`, `bom_fmt_presets`), each a grid
// of names with a Delete button (`OnDeleteBomPreset`, `OnDeleteBomFmtPreset`); the page edits nothing else. Apply (`TransferDataFromWindow`) keeps the presets
// left, one undoable `set_bom_presets`. The studio's Symbol Fields Table offers KiCad's built-in views and saves no preset of its own, so the presets listed
// are those an imported project brought.
import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchSchSetup } from "../../api/client";
import type { SchSetupReply } from "../../api/types";
import { useStudioState } from "../../state/store";
import { PageFrame, usePageApply, usePageDraft, type PageProps } from "../boardSetup/fields";

interface Draft {
  bom: string[];
  fmt: string[];
}

export function BomPresetsPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const [info, setInfo] = useState<SchSetupReply | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [selBom, setSelBom] = useState<number | null>(null);
  const [selFmt, setSelFmt] = useState<number | null>(null);
  const load = useCallback(async () => {
    try {
      const r = await fetchSchSetup();
      if (r.ok) {
        setInfo(r);
        setProblem(null);
      } else setProblem(r.message ?? "The backend could not read the BOM presets.");
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load, state.version]);
  const source = useMemo<Draft | undefined>(() => (info ? { bom: info.bom_presets, fmt: info.bom_fmt_presets } : undefined), [info]);
  const page = usePageDraft<Draft>(source);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const d = page.draft;

  const grid = (title: string, names: string[], sel: number | null, setSel: (i: number | null) => void, del: () => void) => (
    <>
      <div className="bs-section">{title}</div>
      <table className="bs-grid" aria-label={title}>
        <thead>
          <tr>
            <th>Name</th>
          </tr>
        </thead>
        <tbody>
          {names.map((n, i) => (
            <tr key={`${n}-${i}`} className={sel === i ? "bs-sel" : undefined} onClick={() => setSel(i)}>
              <td style={{ minWidth: 240 }}>{n}</td>
            </tr>
          ))}
          {names.length === 0 && (
            <tr>
              <td className="bs-note">(none)</td>
            </tr>
          )}
        </tbody>
      </table>
      <div className="bs-toolbar">
        <button aria-label={`Delete ${title}`} disabled={sel === null || sel >= names.length} onClick={del}>
          Delete
        </button>
      </div>
    </>
  );

  return (
    <PageFrame
      title="BOM Presets"
      hidden={hidden}
      dirty={page.dirty}
      busy={apply.busy}
      message={apply.message?.text ?? problem}
      bad={apply.message?.bad || problem !== null}
      onApply={() => {
        if (!d) return;
        if (!page.dirty) return;
        void apply.run({ op: "set_bom_presets", bom_presets: d.bom, bom_fmt_presets: d.fmt }, () => {
          page.markClean();
          void load();
        });
      }}
      onRevert={() => (page.revert(), apply.setMessage(null))}
    >
      {!d ? (
        <p className="bs-note">{problem ?? "Reading the BOM presets…"}</p>
      ) : (
        <>
          {grid("Bill of Materials Presets", d.bom, selBom, setSelBom, () => {
            if (selBom === null) return;
            page.edit((x) => ({ ...x, bom: x.bom.filter((_, j) => j !== selBom) }));
            setSelBom(null);
          })}
          {grid("Bill of Materials Formatting Presets", d.fmt, selFmt, setSelFmt, () => {
            if (selFmt === null) return;
            page.edit((x) => ({ ...x, fmt: x.fmt.filter((_, j) => j !== selFmt) }));
            setSelFmt(null);
          })}
        </>
      )}
    </PageFrame>
  );
}
