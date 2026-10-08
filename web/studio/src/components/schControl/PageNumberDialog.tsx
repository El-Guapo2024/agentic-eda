// Edit Sheet Page Number... (`eeschema.EditorControl.editPageNumber`, `SCH_EDIT_TOOL::EditPageNumber`): a text entry for the page of one sheet,
// letters and digits only (`wxFILTER_ALPHANUMERIC`: "No white space").
import { useEffect, useState } from "react";
import { useStudioApi, useStudioDispatch } from "../../state/store";
import { fetchHierarchy, type HierarchyEntry } from "../../api/schControlClient";
import { effectivePage, samePath } from "../../kicad-port/sheetPages";
import { SchDialogFrame } from "./SchDialogFrame";

export function PageNumberDialog({ path, onClose }: { path: string[]; onClose: () => void }) {
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const [sheets, setSheets] = useState<HierarchyEntry[] | null>(null);
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    fetchHierarchy().then(
      (list) => {
        if (!alive) return;
        setSheets(list);
        const at = list.findIndex((h) => samePath(h.path, path));
        if (at >= 0) setValue(effectivePage(list[at]!, at));
      },
      (e) => alive && setError(e instanceof Error ? e.message : String(e))
    );
    return () => {
      alive = false;
    };
  }, [path]);

  const entry = sheets?.find((h) => samePath(h.path, path));
  const label = entry ? `/${entry.name}` : "";
  const valid = /^[\p{L}\p{N}]*$/u.test(value);

  const submit = async () => {
    if (!entry || !sheets || !valid) return;
    const index = sheets.findIndex((h) => samePath(h.path, path));
    // `dlg.GetValue() == instance.GetPageNumber()`: nothing to change
    if (value === effectivePage(entry, index)) return onClose();
    const id = path[path.length - 1];
    if (!id) return;
    if (await api.cmd({ op: "set_sheet_page", sheet: id, page: value })) {
      dispatch({ type: "TOAST", message: `Sheet ${label} is page ${value || "(by order)"}.`, kind: "info" });
      onClose();
    }
  };

  return (
    <SchDialogFrame
      title="Edit Sheet Page Number"
      width={400}
      onClose={onClose}
      footer={
        <button className="primary" disabled={!entry || !valid} onClick={() => void submit()}>
          OK
        </button>
      }
    >
      {error && <div className="problem-row">{error}</div>}
      <p style={{ marginTop: 0 }}>Enter page number for sheet path {label || "…"}</p>
      <input autoFocus style={{ width: "100%", boxSizing: "border-box" }} value={value} disabled={!entry} onChange={(e) => setValue(e.target.value.trim())} onKeyDown={(e) => e.key === "Enter" && void submit()} />
      {!valid && <div style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 6 }}>A page number is letters and digits only.</div>}
    </SchDialogFrame>
  );
}
