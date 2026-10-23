// The Embedded Files panel (`PANEL_EMBEDDED_FILES`, common/dialogs/panel_embedded_files.cpp): Board Setup > Board Data > Embedded Files, Schematic Setup > Schematic Data >
// Embedded Files, and the Embedded Files tab of Footprint Properties (on the board and in the Footprint Editor). The rules are kicad-port/embeddedFiles.ts.
//
//   - The grid: Filename and Embedded Reference (`kicad-embed://name`, which a 3D model, a datasheet field or a drawing sheet names the file by), with the type and size the server
//     reads from the file. "Copy Embedded Reference" (`EMBEDDED_FILES_GRID_TRICKS`) copies the link. Board Setup lists the files only footprints hold too (`m_inheritedFiles`), marked.
//   - Add Files (`onAddEmbeddedFiles`): the file chooser, then each file is `add`ed -- a name already embedded asks "Replace" or "Reuse Existing" (`AddEmbeddedFile`).
//   - Remove (`onDeleteEmbeddedFile`): the selected rows; an inherited one is refused, and Board Setup asks whether to take a file out of the footprints that list it too.
//   - Export (`onExportFiles`): the selected rows, or all, downloaded -- a browser saves where its downloads go, so there is no folder to pick.
//   - Embed fonts (`m_cbEmbedFonts`, `onFontEmbedClick`): the fonts the text uses are embedded on save; turning it off drops the ones that were.
//
// Every Add, Remove and Embed fonts is one `Cmd::Embedded` verb, one undo step (KiCad applies the panel on OK; see the module doc of kicad-port/embeddedFiles.ts).
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Cmd } from "../api/types";
import type { EmbeddedListing, EmbeddedOwner } from "../api/embeddedTypes";
import { embeddedFileUrl, fetchEmbedded, readFileBase64 } from "../api/embeddedClient";
import { addCmd, addDecision, fontsCmd, NESTED_QUESTION, removeCmd, removePlan, rowsOf, sizeText } from "../kicad-port/embeddedFiles";
import { setActiveEmbeddedPanel, type ActiveEmbeddedPanel } from "../state/embeddedPanel";

export interface EmbeddedFilesPanelProps {
  owner: EmbeddedOwner;
  /** Sends one verb (the editor's own `cmd`, which refreshes the document and says why a refusal was refused). */
  run: (cmd: Cmd) => Promise<boolean>;
  hidden?: boolean;
  /** Changes when the document does (the board's version): the list is read again, so an undo from outside shows. */
  version?: string | null;
  /** Show the "Embed fonts" box (KiCad has it on every owner's panel). */
  fonts?: boolean;
}

/** A question the panel asks in its own footer (no native dialog). */
type Question = { text: string; yes: string; no: string; onYes: () => void; onNo: () => void } | null;

export function EmbeddedFilesPanel({ owner, run, hidden = false, version = null, fonts = true }: EmbeddedFilesPanelProps) {
  const [listing, setListing] = useState<EmbeddedListing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const [question, setQuestion] = useState<Question>(null);
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLInputElement | null>(null);
  const ownerKey = JSON.stringify(owner);

  const reload = useCallback(async () => {
    try {
      setListing(await fetchEmbedded(owner));
      setError(null);
    } catch (e) {
      setListing(null);
      setError(e instanceof Error ? e.message : String(e));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ownerKey]);

  useEffect(() => {
    if (!hidden) void reload();
  }, [hidden, reload, version]);

  useEffect(() => setSelected(new Set()), [ownerKey]);

  const rows = useMemo(() => rowsOf(listing), [listing]);
  const send = async (cmd: Cmd): Promise<boolean> => {
    setBusy(true);
    try {
      const ok = await run(cmd);
      await reload();
      return ok;
    } finally {
      setBusy(false);
    }
  };

  // Add Files: one verb per file; a name that is taken asks first.
  const addFiles = async (files: readonly File[]) => {
    setMessage(null);
    for (const file of files) {
      const decision = addDecision(listing, file.name, file.size);
      if (decision.kind === "refuse") {
        setMessage(decision.message);
        continue;
      }
      let data: string;
      try {
        data = await readFileBase64(file);
      } catch {
        setMessage(`Failed to read file '${file.name}'.`);
        continue;
      }
      if (decision.kind === "conflict") {
        const replace = await new Promise<boolean>((resolve) =>
          setQuestion({ text: decision.message, yes: "Replace", no: "Reuse Existing", onYes: () => resolve(true), onNo: () => resolve(false) })
        );
        setQuestion(null);
        if (!replace) continue; // "Reuse Existing": the file that is there stays
        await send(addCmd(owner, file.name, data, true));
      } else {
        await send(addCmd(owner, file.name, data));
      }
    }
  };

  const remove = async (names: readonly string[]) => {
    setMessage(null);
    const plan = removePlan(owner, rows, names);
    if ("error" in plan) {
      setMessage(plan.error);
      return;
    }
    let nested = false;
    if (plan.askNested) {
      nested = await new Promise<boolean>((resolve) => setQuestion({ text: NESTED_QUESTION, yes: "OK", no: "Cancel", onYes: () => resolve(true), onNo: () => resolve(false) }));
      setQuestion(null);
    }
    if (await send(removeCmd(owner, plan.names, nested))) setSelected(new Set());
  };

  // Export: a download per file, from the route that decodes it.
  const extract = (names: readonly string[]) => {
    setMessage(null);
    const list = names.length > 0 ? names : rows.map((r) => r.name);
    for (const name of list) {
      const a = document.createElement("a");
      a.href = embeddedFileUrl(owner, name);
      a.download = name;
      a.rel = "noopener";
      document.body.appendChild(a);
      a.click();
      a.remove();
    }
    if (list.length === 0) setMessage("There are no files to export.");
  };

  const copyLink = (link: string) => {
    void navigator.clipboard?.writeText(link).then(
      () => setMessage(`Copied ${link}`),
      () => setMessage(link)
    );
  };

  // The actions `common.Embed.removeFile` and `extractFile` act on the panel that is showing.
  const removeRef = useRef(remove);
  const extractRef = useRef(extract);
  removeRef.current = remove;
  extractRef.current = extract;
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  useEffect(() => {
    if (hidden) return;
    const me: ActiveEmbeddedPanel = { owner, remove: () => void removeRef.current([...selectedRef.current]), extract: () => extractRef.current([...selectedRef.current]) };
    setActiveEmbeddedPanel(me);
    return () => setActiveEmbeddedPanel(null, me);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hidden, ownerKey]);

  const toggle = (name: string, extend: boolean) =>
    setSelected((prev) => {
      const next = new Set(extend ? prev : []);
      if (extend && prev.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });

  return (
    <section className="bs-page" hidden={hidden} aria-label="Embedded Files" data-testid="embedded-files-panel">
      <h3 className="bs-title">Embedded Files</h3>
      {error && <p className="bs-error">{error}</p>}
      <div className="bs-scroll-x">
        <table className="bs-grid" aria-label="Embedded files" data-testid="embedded-files-grid">
          <thead>
            <tr>
              <th>Filename</th>
              <th>Embedded Reference</th>
              <th>Type</th>
              <th>Size</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr
                key={r.name}
                data-name={r.name}
                aria-selected={selected.has(r.name)}
                className={selected.has(r.name) ? "selected" : undefined}
                style={{ cursor: "pointer", background: selected.has(r.name) ? "var(--chrome-selection, rgba(74,144,226,0.25))" : undefined }}
                onClick={(e) => toggle(r.name, e.ctrlKey || e.metaKey || e.shiftKey)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  copyLink(r.link);
                }}
              >
                <td>
                  {r.name}
                  {r.inherited && (
                    <span className="bs-note" title={`Embedded in ${r.used_by.join(", ")}`}>
                      {" "}
                      (from {r.used_by.join(", ")})
                    </span>
                  )}
                </td>
                <td>
                  <code>{r.link}</code>
                </td>
                <td>{r.type_label}</td>
                <td title={r.status === "error" || r.status === "legacy" ? r.message : undefined} className={r.status === "error" ? "bs-error" : undefined}>
                  {r.status === "error" ? "unreadable" : r.status === "reference" ? "" : sizeText(r.size)}
                </td>
                <td>
                  <button title="Copy the reference for this embedded file" onClick={(e) => (e.stopPropagation(), copyLink(r.link))}>
                    Copy Embedded Reference
                  </button>
                </td>
              </tr>
            ))}
            {rows.length === 0 && (
              <tr>
                <td colSpan={5} className="bs-note">
                  No embedded files.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
      <div className="bs-toolbar" style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <input
          ref={input}
          type="file"
          multiple
          hidden
          data-testid="embedded-files-input"
          onChange={(e) => {
            const files = [...(e.target.files ?? [])];
            e.target.value = "";
            void addFiles(files);
          }}
        />
        <button disabled={busy || !!error} title="Select a file to embed" onClick={() => input.current?.click()} data-testid="embedded-files-add">
          Add Files...
        </button>
        <button disabled={busy || selected.size === 0} title="Remove an embedded file" onClick={() => void remove([...selected])} data-testid="embedded-files-remove">
          Remove
        </button>
        <button disabled={busy || rows.length === 0} title="Extract an embedded file" onClick={() => extract([...selected])} data-testid="embedded-files-export">
          Export...
        </button>
        {fonts && (
          <label style={{ marginLeft: "auto" }}>
            <input type="checkbox" checked={!!listing?.fonts} disabled={busy || !listing} onChange={(e) => void send(fontsCmd(owner, e.target.checked))} data-testid="embedded-files-fonts" /> Embed fonts
          </label>
        )}
      </div>
      {question && (
        <div className="bs-toolbar" role="alertdialog" aria-label="Embedded File Conflict" style={{ display: "flex", gap: 8, alignItems: "center" }} data-testid="embedded-files-question">
          <span style={{ whiteSpace: "pre-line", marginRight: "auto" }}>{question.text}</span>
          <button className="primary" onClick={question.onYes}>
            {question.yes}
          </button>
          <button onClick={question.onNo}>{question.no}</button>
        </div>
      )}
      {message && (
        <p className="bs-note" role="status" data-testid="embedded-files-message" style={{ whiteSpace: "pre-line" }}>
          {message}
        </p>
      )}
    </section>
  );
}
