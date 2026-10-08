// Generate Legacy Bill of Materials... (`eeschema.EditorControl.generateBOMLegacy`, `InvokeDialogCreateBOM` -> DIALOG_BOM): the BOM generator scripts KiCad ships
// (the plugins folder of the installed KiCad), the one picked, the command it runs and what it printed. Generate writes the intermediate XML netlist
// (`kicad-cli sch export python-bom`) and runs the script over it, into this board's export/kicad/sch-python-bom/ folder; the browser can then save a copy.
// Not ported: adding a script of your own, removing one, editing its command line and the console flag -- the server runs only KiCad's own scripts.
import { useEffect, useState } from "react";
import { fetchBomPlugins, postBomLegacy, type BomPlugin } from "../../api/schControlClient";
import { saveTextFile } from "../../api/libraryClient";
import { SchDialogFrame } from "./SchDialogFrame";

const STORAGE_KEY = "eda-studio.bom-plugin";

function remembered(): string | null {
  try {
    return window.localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

function remember(file: string): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, file);
  } catch {
    /* blocked site data: the pick is simply not kept */
  }
}

export function LegacyBomDialog({ onClose }: { onClose: () => void }) {
  const [plugins, setPlugins] = useState<BomPlugin[] | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // The last run: what the generator said, the files written and the output to save.
  const [run, setRun] = useState<{ ok: boolean; text: string; files: string[]; output?: { name: string; text: string } } | null>(null);

  useEffect(() => {
    let alive = true;
    fetchBomPlugins().then(
      ({ plugins: list }) => {
        if (!alive) return;
        setPlugins(list);
        const want = remembered();
        setPicked(list.find((p) => p.file === want)?.file ?? list[0]?.file ?? null);
      },
      (e) => alive && setProblem(e instanceof Error ? e.message : String(e))
    );
    return () => {
      alive = false;
    };
  }, []);

  const plugin = plugins?.find((p) => p.file === picked) ?? null;

  const pick = (file: string) => {
    setPicked(file);
    setRun(null);
    remember(file);
  };

  const generate = async () => {
    if (!plugin || busy) return;
    setBusy(true);
    setRun(null);
    try {
      const r = await postBomLegacy(plugin.file);
      const said = (r.messages ?? "").trim();
      const wrote = (r.files ?? []).map((f) => `Wrote ${f}`).join("\n");
      setRun({ ok: r.ok, text: [r.ok ? "" : (r.message ?? "Failed to create file."), said, wrote].filter((t) => t !== "").join("\n\n"), files: r.files ?? [], output: r.output });
    } catch (e) {
      setRun({ ok: false, text: `Failed to create file.\n\n${e instanceof Error ? e.message : String(e)}`, files: [] });
    } finally {
      setBusy(false);
    }
  };

  const box = { border: "1px solid var(--chrome-border)", height: "32vh", overflow: "auto", fontSize: 12 };
  return (
    <SchDialogFrame
      title="Bill of Materials"
      width={820}
      onClose={onClose}
      footer={
        <>
          {run?.ok && run.output && (
            <button onClick={() => saveTextFile(run.output!.text, run.output!.name)} title="Save the generated file from the browser">
              Save a copy...
            </button>
          )}
          <button className="primary" disabled={!plugin || busy} onClick={() => void generate()}>
            {busy ? "Generating..." : "Generate"}
          </button>
        </>
      }
    >
      {problem && <div className="panel-empty">{problem}</div>}
      {!problem && plugins === null && <div className="panel-empty">Looking for KiCad's BOM generator scripts...</div>}
      {plugins && plugins.length === 0 && <div className="panel-empty">KiCad's plugins folder holds no BOM generator scripts.</div>}
      {plugins && plugins.length > 0 && (
        <div style={{ display: "flex", gap: 10, alignItems: "stretch" }}>
          <div style={{ flex: "0 0 250px", minWidth: 0 }}>
            <div style={{ fontWeight: 600, fontSize: 11, marginBottom: 4 }}>BOM generator scripts</div>
            <div style={box}>
              {plugins.map((p) => (
                <div
                  key={p.file}
                  title={p.file}
                  onClick={() => pick(p.file)}
                  style={{ padding: "2px 6px", cursor: "default", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", background: p.file === picked ? "var(--chrome-selected-bg)" : undefined, color: p.file === picked ? "var(--chrome-selected-text)" : undefined }}
                >
                  {p.name}
                </div>
              ))}
            </div>
          </div>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div className="kv-grid" style={{ gridTemplateColumns: "100px 1fr" }}>
              <span>Name</span>
              <input readOnly value={plugin?.name ?? ""} />
              <span>Command line</span>
              <input readOnly value={plugin?.command ?? ""} title={plugin?.command ?? ""} style={{ fontFamily: "monospace", fontSize: 11 }} />
            </div>
            <div style={{ fontWeight: 600, fontSize: 11, margin: "8px 0 4px" }}>Messages</div>
            <textarea
              readOnly
              spellCheck={false}
              value={run ? run.text : (plugin?.info ?? "")}
              style={{ width: "100%", boxSizing: "border-box", height: "24vh", fontFamily: "monospace", fontSize: 11, whiteSpace: "pre", color: run && !run.ok ? "var(--chrome-error, #d9534f)" : undefined }}
            />
          </div>
        </div>
      )}
    </SchDialogFrame>
  );
}
