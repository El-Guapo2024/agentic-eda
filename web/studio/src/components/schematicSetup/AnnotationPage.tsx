// Schematic Setup > General > Annotation -- port of eeschema/dialogs/panel_eeschema_annotation_options.cpp (`loadEEschemaSettings`, `TransferDataFromWindow`):
//
//   Units      Symbol unit notation: A, .A, -A, _A, .1, -1, _1 -- the first unit's id ('A' or '1', `m_SubpartFirstId`) and the separator (`m_SubpartIdSeparator`),
//              the project's `subpart_first_id` / `subpart_id_separator`, which name every unit's reference (U1A, U1.1) and so the nets of its pins
//   Order      Sort symbols by X position / by Y position (`m_AnnotateSortOrder`)
//   Numbering  Use first free number after: [n] / First free after sheet number X 100 / X 1000 (`m_AnnotateMethod`, `m_AnnotateStartNum`), and
//              "Allow reference reuse" (`REFDES_TRACKER::SetReuseRefDes`)
//
// The Annotate dialog starts from the order, the numbering and the start number (`DIALOG_ANNOTATE::TransferDataToWindow`) and leaves its choice in them when it
// closes (`~DIALOG_ANNOTATE`). Apply is one undoable `set_annotation_settings`. The start number is read as the Annotate dialog reads it (`startNumber`).
import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchSchSetup } from "../../api/client";
import type { AnnotationSettings, SchSetupReply } from "../../api/types";
import { rememberAnnotateSettings, rememberedAnnotateSettings, startNumber } from "../../kicad-port/schAnnotate";
import { DEFAULT_ANNOTATION, UNIT_NOTATIONS, unitNotationChoice } from "../../kicad-port/schSetup";
import { useStudioState } from "../../state/store";
import { Check, FieldRow, PageFrame, usePageApply, usePageDraft, type PageProps, useSchSetupImport } from "../boardSetup/fields";

interface Draft {
  notation: number;
  sort: number;
  method: number;
  start: string;
  reuse: boolean;
}

function draftOf(r: SchSetupReply): Draft {
  const a = r.annotation ?? DEFAULT_ANNOTATION;
  return { notation: unitNotationChoice(r.subpart_first_id, r.subpart_id_separator), sort: a.sort_order, method: a.method, start: String(a.start_num), reuse: a.reuse_designators };
}

export function AnnotationPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const [info, setInfo] = useState<SchSetupReply | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const load = useCallback(async () => {
    try {
      const r = await fetchSchSetup();
      if (r.ok) {
        setInfo(r);
        setProblem(null);
      } else setProblem(r.message ?? "The backend could not read the schematic's annotation settings.");
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load, state.version]);

  const source = useMemo(() => (info ? draftOf(info) : undefined), [info]);
  const page = usePageDraft<Draft>(source);
  // "Import Settings from Another Project..." (`ImportSettingsFrom`, UI-LEFT-2 BB4)
  useSchSetupImport("annotation", (s) => draftOf(s.setup), page.importDraft);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const d = page.draft;
  const set = (patch: Partial<Draft>) => page.edit((x) => ({ ...x, ...patch }));

  const onApply = () => {
    if (!d) return;
    // nothing edited: nothing to store (the verb refuses a change that changes nothing)
    if (!page.dirty) return;
    const notation = UNIT_NOTATIONS[d.notation] ?? UNIT_NOTATIONS[0]!;
    const settings: AnnotationSettings = { sort_order: d.sort, method: d.method, start_num: startNumber(d.start), reuse_designators: d.reuse };
    void apply.run({ op: "set_annotation_settings", settings, subpart_first_id: notation.firstId, subpart_id_separator: notation.separator }, () => {
      page.markClean();
      // what Paste and Import Sheet number by until the Annotate dialog is opened again (`SCH_EDITOR_CONTROL::Paste` reads these settings)
      rememberAnnotateSettings({ ...rememberedAnnotateSettings(), sort: d.sort === 1 ? "y" : "x", algo: d.method === 1 ? "sheet_x100" : d.method === 2 ? "sheet_x1000" : "first_free", start: String(settings.start_num) });
      void load();
    });
  };

  const radio = (name: string, checked: boolean, label: string, onPick: () => void) => (
    <label className="bs-check">
      <input type="radio" name={name} aria-label={label} checked={checked} onChange={onPick} /> {label}
    </label>
  );

  return (
    <PageFrame title="Annotation" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? problem} bad={apply.message?.bad || problem !== null} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!d ? (
        <p className="bs-note">{problem ?? "Reading the schematic's annotation settings…"}</p>
      ) : (
        <>
          <h4 className="bs-subhead">Units</h4>
          <FieldRow label="Symbol unit notation:" title="SCHEMATIC_SETTINGS::m_SubpartFirstId and m_SubpartIdSeparator: how a unit is named after its reference (U1A, U1.A, U1-1 ...)">
            <select aria-label="Symbol unit notation" value={d.notation} onChange={(e) => set({ notation: Number(e.target.value) })}>
              {UNIT_NOTATIONS.map((n, i) => (
                <option key={n.label} value={i}>
                  {n.label}
                </option>
              ))}
            </select>
          </FieldRow>
          <h4 className="bs-subhead">Order</h4>
          {radio("sch-setup-order", d.sort === 0, "Sort symbols by X position", () => set({ sort: 0 }))}
          {radio("sch-setup-order", d.sort === 1, "Sort symbols by Y position", () => set({ sort: 1 }))}
          <h4 className="bs-subhead">Numbering</h4>
          <label className="bs-check">
            <input type="radio" name="sch-setup-method" aria-label="Use first free number after:" checked={d.method === 0} onChange={() => set({ method: 0 })} /> Use first free number after:{" "}
            <input className="bs-input" aria-label="First free number after" style={{ width: 64 }} value={d.start} onChange={(e) => set({ start: e.target.value })} />
          </label>
          {radio("sch-setup-method", d.method === 1, "First free after sheet number X 100", () => set({ method: 1 }))}
          {radio("sch-setup-method", d.method === 2, "First free after sheet number X 1000", () => set({ method: 2 }))}
          <Check checked={d.reuse} onChange={(v) => set({ reuse: v })} title="REFDES_TRACKER::GetReuseRefDes: a reference a deleted symbol had may be given again">
            Allow reference reuse
          </Check>
        </>
      )}
    </PageFrame>
  );
}
