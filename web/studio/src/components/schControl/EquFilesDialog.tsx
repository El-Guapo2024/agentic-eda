// Manage Footprint Association Files... (`cvpcb.Control.ShowEquFileTable` -> `DIALOG_CONFIG_EQUFILES`, KiCad 25ec2335 cvpcb/dialogs/dialog_config_equfiles.cpp): the project's
// list of footprint association files (`.equ`, `cvpcb.equivalence_files`), which Automatically Assign Footprints reads. "Footprint association files:" with Add, Move up,
// Move down, Edit and Remove, the "Available path substitutions:" grid (`KIPRJMOD`, the footprint library folder's variable), OK and Cancel. OK saves the list at once,
// no undo step, as KiCad's `SaveProject` (the board folder's `cvpcb.json`, crates/cli/src/cvpcb_api.rs). Add's file dialog is a browser over the server's folders,
// starting in the folder of the grid's selected variable (a page cannot see the machine's), and Edit cannot start a text editor from a page: it says so. The rules are
// kicad-port/cvpcbEqu.ts's.
import { useEffect, useState } from "react";
import { browseEqu, fetchEquFiles, saveEquFiles, type EquBrowse, type EquFilesView } from "../../api/cvpcbClient";
import { addEquFiles, moveEquDown, moveEquUp, removeEqu } from "../../kicad-port/cvpcbEqu";
import { SchDialogFrame } from "./SchDialogFrame";

export function EquFilesDialog({ onClose, onSaved }: { onClose: () => void; onSaved?: (view: EquFilesView) => void }) {
  const [view, setView] = useState<EquFilesView | null>(null);
  const [list, setList] = useState<string[]>([]);
  const [selected, setSelected] = useState<number[]>([]);
  const [varRow, setVarRow] = useState(0);
  const [message, setMessage] = useState<string | null>(null);
  const [browse, setBrowse] = useState<EquBrowse | null>(null);
  const [picked, setPicked] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    fetchEquFiles().then(
      (v) => {
        if (!alive) return;
        setView(v);
        setList(v.files);
      },
      (e) => alive && setMessage(e instanceof Error ? e.message : String(e))
    );
    return () => {
      alive = false;
    };
  }, []);

  const vars = view?.vars ?? [];
  const found = (entry: string) => view?.resolved.find((r) => r.entry === entry);

  const openFolder = (path: string) => {
    browseEqu(path).then(
      (b) => {
        if (b.error) setMessage(b.error);
        else {
          setBrowse(b);
          setPicked([]);
        }
      },
      (e) => setMessage(e instanceof Error ? e.message : String(e))
    );
  };

  /** `OnAddFiles`: the file dialog opens in the folder of the grid's selected row. */
  const add = () => {
    setMessage(null);
    openFolder(vars[varRow]?.value ?? "");
  };

  const takePicked = () => {
    if (!browse) return;
    const paths = picked.map((name) => `${browse.path.replace(/\/$/, "")}/${name}`);
    const r = addEquFiles(list, paths, vars);
    setList(r.list);
    setBrowse(null);
    if (r.errors.length > 0) setMessage(r.errors.join("\n"));
  };

  const ok = async () => {
    setBusy(true);
    try {
      const v = await saveEquFiles(list);
      if (v.error) return setMessage(v.error);
      onSaved?.(v);
      onClose();
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const click = (i: number, e: React.MouseEvent) => setSelected((s) => (e.ctrlKey || e.metaKey ? (s.includes(i) ? s.filter((x) => x !== i) : [...s, i]) : [i]));
  const box = { border: "1px solid var(--chrome-border)", overflow: "auto", fontSize: 12 } as const;
  const row = (on: boolean) => ({ padding: "2px 6px", cursor: "default", whiteSpace: "nowrap" as const, background: on ? "var(--chrome-selected-bg)" : undefined, color: on ? "var(--chrome-selected-text)" : undefined });

  return (
    <SchDialogFrame
      title={view ? `Project file: '${view.project_file}'` : "Footprint Association Files"}
      width={640}
      onClose={() => (browse ? setBrowse(null) : onClose())}
      footer={
        <button className="primary" disabled={busy || !view} onClick={() => void ok()} data-equ-ok>
          OK
        </button>
      }
    >
      <div data-equ-dialog style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <div style={{ fontSize: 12 }}>Footprint association files:</div>
        <div style={{ display: "flex", gap: 6 }}>
          <div style={{ ...box, flex: 1, height: 150 }} role="listbox" aria-multiselectable aria-label="Footprint association files">
            {list.map((f, i) => {
              const r = found(f);
              return (
                <div key={`${f}-${i}`} role="option" aria-selected={selected.includes(i)} data-equ-entry={f} style={row(selected.includes(i))} onClick={(e) => click(i, e)} title={r ? `${r.path}${r.found ? "" : " (not found)"}` : undefined}>
                  {f}
                  {r && !r.found && <span style={{ opacity: 0.6 }}> (not found)</span>}
                </div>
              );
            })}
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            <button title="Add association file" aria-label="Add association file" onClick={add}>
              {"📂"}
            </button>
            <button title="Move up" aria-label="Move up" onClick={() => ((r) => (setList(r.list), setSelected(r.selected)))(moveEquUp(list, selected))}>
              {"↑"}
            </button>
            <button title="Move down" aria-label="Move down" onClick={() => ((r) => (setList(r.list), setSelected(r.selected)))(moveEquDown(list, selected))}>
              {"↓"}
            </button>
            <button
              title="Edit association file"
              aria-label="Edit association file"
              disabled={selected.length === 0}
              onClick={() => setMessage(`The studio cannot start a text editor from the page: edit ${selected.map((i) => found(list[i] ?? "")?.path ?? list[i]).join(", ")} outside it.`)}
            >
              {"✎"}
            </button>
            <button
              title="Remove association file"
              aria-label="Remove association file"
              onClick={() => {
                setList(removeEqu(list, selected));
                setSelected([]);
              }}
            >
              {"🗑"}
            </button>
          </div>
        </div>
        {message && (
          <div role="alert" data-equ-message style={{ whiteSpace: "pre-wrap", fontSize: 12, color: "var(--chrome-error, #c33)" }}>
            {message}
          </div>
        )}
        {browse && (
          <div data-equ-browse style={{ border: "1px solid var(--chrome-border)", padding: 6, fontSize: 12 }}>
            <div style={{ fontWeight: 600, marginBottom: 4 }}>Footprint Association File</div>
            <div style={{ display: "flex", gap: 6, alignItems: "center", marginBottom: 4 }}>
              <button disabled={!browse.parent} onClick={() => browse.parent && openFolder(browse.parent)} title="Up one folder">
                {"⇧"}
              </button>
              <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={browse.path}>
                {browse.path}
              </span>
            </div>
            <div style={{ ...box, height: 140 }}>
              {browse.entries.map((e) => (
                <div
                  key={e.name}
                  data-equ-file={e.dir ? undefined : e.name}
                  style={row(picked.includes(e.name))}
                  onClick={(ev) => (e.dir ? undefined : setPicked((p) => (ev.ctrlKey || ev.metaKey ? (p.includes(e.name) ? p.filter((x) => x !== e.name) : [...p, e.name]) : [e.name])))}
                  onDoubleClick={() => (e.dir ? openFolder(`${browse.path.replace(/\/$/, "")}/${e.name}`) : undefined)}
                >
                  {e.dir ? `📁 ${e.name}` : e.name}
                </div>
              ))}
              {browse.entries.length === 0 && <div style={{ opacity: 0.6, padding: 4 }}>No folders or .equ files here.</div>}
            </div>
            <div style={{ display: "flex", justifyContent: "flex-end", gap: 6, marginTop: 4 }}>
              <button onClick={() => setBrowse(null)}>Cancel</button>
              <button className="primary" disabled={picked.length === 0} onClick={takePicked} data-equ-open>
                Open
              </button>
            </div>
          </div>
        )}
        <div style={{ fontSize: 12 }}>Available path substitutions:</div>
        <table style={{ ...box, borderCollapse: "collapse", width: "100%" }} aria-label="Available path substitutions">
          <thead>
            <tr>
              <th style={{ textAlign: "left", padding: "2px 6px" }}>Name</th>
              <th style={{ textAlign: "left", padding: "2px 6px" }}>Value</th>
            </tr>
          </thead>
          <tbody>
            {vars.map((v, i) => (
              <tr key={v.name} style={row(varRow === i)} onClick={() => setVarRow(i)}>
                <td style={{ padding: "2px 6px" }}>{v.name}</td>
                <td style={{ padding: "2px 6px" }}>{v.value}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </SchDialogFrame>
  );
}
