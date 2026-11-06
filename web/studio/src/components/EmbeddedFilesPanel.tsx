// The Embedded Files panel (`PANEL_EMBEDDED_FILES`, common/dialogs/panel_embedded_files.cpp): Board Setup > Board Data > Embedded Files, Schematic Setup > Schematic Data >
// Embedded Files, and the Embedded Files tab of Footprint Properties (on the board and in the Footprint Editor). The rules are kicad-port/embeddedFiles.ts.
//
//   - The panel edits a copy of the list (`m_localFiles`, an `EmbeddedDraft` over the list the server gives) and writes it back on OK (`TransferDataFromWindow`) as one `apply`
//     verb, one undo step: the dialog's OK in Footprint Properties (the dialog holds the copy, `draft` / `onDraft`, and puts the verb in its batch), the page's Apply in
//     Board Setup and Schematic Setup, whose pages each apply on their own (the page is marked not applied until then, and Close asks before it drops the copy).
//   - The grid: Filename and Embedded Reference (`kicad-embed://name`, which a 3D model, a datasheet field or a drawing sheet names the file by), with the type and size the server
//     reads from the file; a row not applied yet is marked. "Copy Embedded Reference" (`EMBEDDED_FILES_GRID_TRICKS`) copies the link. Board Setup lists the files only
//     footprints hold too (`m_inheritedFiles`), marked.
//   - Add Files (`onAddEmbeddedFiles`): the file chooser, then each file goes into the copy -- a name the copy has with the same content is reused without a word, with other
//     content asks "Replace", "Reuse Existing" or "Cancel" (`AddEmbeddedFile`).
//   - Remove (`onDeleteEmbeddedFile`): the selected rows out of the copy; an inherited one is refused. Board Setup's OK asks whether to take a file out of the footprints that
//     list it too ("Deleted embedded files are also referenced in some footprints.\nDelete from footprints as well?").
//   - Export (`onExportFiles`): every file of the copy into a folder picked once ("Select a directory to export files"); a file that is there asks "File '%s' already exists."
//     (Overwrite / Skip, "Do not show again"). Where the browser has no folder picker (the File System Access API), each file is downloaded instead -- there is then no folder
//     to choose and no overwrite question.
//   - Embed fonts (`m_cbEmbedFonts`, `onFontEmbedClick`): a click drops the fonts of the copy and, ticked, takes in the fonts the text uses (`GET /api/embedded/fonts`); the
//     board embeds them again on every save (`BOARD::EmbedFonts`, crates/kicad-engine/src/fonts.rs).
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Cmd } from "../api/types";
import type { EmbeddedListing, EmbeddedOwner, TextFont } from "../api/embeddedTypes";
import { embeddedFileUrl, fetchEmbedded, fetchTextFonts, readFileBase64, textFontUrl } from "../api/embeddedClient";
import {
  addDecision,
  applyCmd,
  asksNested,
  bytesOfBase64,
  checksumOf,
  CONFLICT_TITLE,
  draftAdd,
  draftDirty,
  draftFontsClick,
  draftFontsOn,
  draftListing,
  draftRemove,
  draftRows,
  EMPTY_DRAFT,
  exportInto,
  exportNames,
  fullNameOf as fullName,
  NESTED_QUESTION,
  NESTED_TITLE,
  removePlan,
  sizeText,
  type EmbeddedDraft,
  type ExportFolder,
} from "../kicad-port/embeddedFiles";
import { setActiveEmbeddedPanel, type ActiveEmbeddedPanel } from "../state/embeddedPanel";

export interface EmbeddedFilesPanelProps {
  owner: EmbeddedOwner;
  /** Sends one verb (the editor's own `cmd`, which refreshes the document and says why a refusal was refused): the page's Apply. */
  run: (cmd: Cmd) => Promise<boolean>;
  hidden?: boolean;
  /** Changes when the document does (the board's version): the list is read again, so an undo from outside shows. */
  version?: string | null;
  /** Show the "Embed fonts" box (KiCad has it on every owner's panel). */
  fonts?: boolean;
  /** A page of Board Setup or Schematic Setup: whether the copy differs from the list (the page's "not applied" mark). */
  onDirty?: (dirty: boolean) => void;
  /** A tab of a dialog with an OK: the dialog holds the copy (so it outlives the tab) and sends it with its OK (`applyCmd`); the panel shows no Apply then. */
  draft?: EmbeddedDraft;
  onDraft?: (draft: EmbeddedDraft, listing: EmbeddedListing | null) => void;
}

/** A question the panel asks in its own footer (no native dialog). `remember` shows KiCad's "Do not show again" box. */
type Question = { title: string; text: string; yes: string; no: string; onYes: (remember: boolean) => void; onNo: (remember: boolean) => void; cancel?: () => void; remember?: boolean } | null;

/** `window.showDirectoryPicker`, where the browser has it. */
type DirectoryPicker = (options?: { mode?: "read" | "readwrite"; id?: string }) => Promise<ExportFolder>;

export function EmbeddedFilesPanel({ owner, run, hidden = false, version = null, fonts = true, onDirty, draft: controlled, onDraft }: EmbeddedFilesPanelProps) {
  const [listing, setListing] = useState<EmbeddedListing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const [question, setQuestion] = useState<Question>(null);
  const [rememberBox, setRememberBox] = useState(false);
  const [busy, setBusy] = useState(false);
  const [ownDraft, setOwnDraft] = useState<EmbeddedDraft>(EMPTY_DRAFT);
  const input = useRef<HTMLInputElement | null>(null);
  const ownerKey = JSON.stringify(owner);
  const inDialog = controlled !== undefined;
  const draft = controlled ?? ownDraft;
  // the draft and the list as they are now, for the async handlers that change the draft one file at a time
  const draftRef = useRef(draft);
  draftRef.current = draft;
  const listingRef = useRef(listing);
  listingRef.current = listing;
  const onDraftRef = useRef(onDraft);
  onDraftRef.current = onDraft;
  const setDraft = useCallback((d: EmbeddedDraft) => {
    draftRef.current = d;
    if (onDraftRef.current) onDraftRef.current(d, listingRef.current);
    else setOwnDraft(d);
  }, []);

  const reload = useCallback(async () => {
    try {
      const l = await fetchEmbedded(owner);
      setListing(l);
      listingRef.current = l;
      setError(null);
      onDraftRef.current?.(draftRef.current, l);
    } catch (e) {
      setListing(null);
      setError(e instanceof Error ? e.message : String(e));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ownerKey]);

  useEffect(() => {
    if (!hidden) void reload();
  }, [hidden, reload, version]);

  useEffect(() => {
    setSelected(new Set());
    if (!inDialog) setOwnDraft(EMPTY_DRAFT);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ownerKey]);

  const dirty = draftDirty(draft);
  useEffect(() => onDirty?.(dirty), [dirty, onDirty]);

  const rows = useMemo(() => draftRows(listing, draft), [listing, draft]);
  // the checksums of the files taken in, for a name taken again (`ComputeFileHash` of the file there)
  const pendingChecksums = useRef(new Map<string, string>());

  const ask = (q: NonNullable<Question>) => {
    setRememberBox(false);
    setQuestion(q);
  };

  // Add Files: each file into the copy; a name the copy has asks first.
  const addFiles = async (files: readonly File[]) => {
    setMessage(null);
    for (const file of files) {
      const name = fullName(file.name);
      let checksum: string | undefined;
      let data: string;
      try {
        // `ComputeFileHash` only for a name that is taken: the content is compared with the file that is there
        const copy = draftListing(listingRef.current, draftRef.current, pendingChecksums.current);
        if (copy?.files.some((f) => f.name === name)) {
          const pending = draftRef.current.added.find((a) => a.name === name);
          if (pending && !pendingChecksums.current.has(name)) pendingChecksums.current.set(name, checksumOf(pending.blob ? new Uint8Array(await pending.blob.arrayBuffer()) : bytesOfBase64(pending.data)));
          checksum = checksumOf(new Uint8Array(await file.arrayBuffer()));
        }
        data = await readFileBase64(file);
      } catch {
        setMessage(`Failed to read file '${name}'.`);
        continue;
      }
      const decision = addDecision(draftListing(listingRef.current, draftRef.current, pendingChecksums.current), file.name, file.size, checksum);
      if (decision.kind === "refuse") {
        setMessage(decision.message);
        continue;
      }
      if (decision.kind === "reuse") continue; // the same content: the file that is there is the one
      if (decision.kind === "conflict") {
        const replace = await new Promise<boolean>((resolve) => ask({ title: CONFLICT_TITLE, text: decision.message, yes: "Replace", no: "Reuse Existing", onYes: () => resolve(true), onNo: () => resolve(false), cancel: () => resolve(false) }));
        setQuestion(null);
        if (!replace) continue; // "Reuse Existing" and "Cancel": the file that is there stays
      }
      pendingChecksums.current.delete(name);
      if (checksum) pendingChecksums.current.set(name, checksum);
      setDraft(draftAdd(draftRef.current, { name, data, size: file.size, blob: file }));
    }
  };

  const remove = (names: readonly string[]) => {
    setMessage(null);
    const plan = removePlan(owner, rows, names);
    if (!("names" in plan)) {
      setMessage(plan.error);
      return;
    }
    let d = draftRef.current;
    for (const n of plan.names) {
      d = draftRemove(listingRef.current, d, n);
      pendingChecksums.current.delete(n);
    }
    setDraft(d);
    setSelected(new Set());
    if (plan.error) setMessage(plan.error);
  };

  // Embed fonts (`onFontEmbedClick`): the fonts of the copy go and, ticked, the fonts the text uses come in.
  const clickFonts = async (checked: boolean) => {
    setMessage(null);
    let used: TextFont[] = [];
    if (checked) {
      try {
        used = (await fetchTextFonts(owner)).fonts;
      } catch (e) {
        setMessage(e instanceof Error ? e.message : String(e));
      }
    }
    setDraft(draftFontsClick(draftRef.current, checked, used));
  };

  // Export (`onExportFiles`): every file of the copy.
  const exportFiles = async (names: readonly string[]) => {
    setMessage(null);
    const list = names.length > 0 ? names : exportNames(rows);
    if (list.length === 0) {
      setMessage("There are no files to export.");
      return;
    }
    const d = draftRef.current;
    const bytesOf = (name: string) => async (): Promise<Uint8Array> => {
      const pending = d.added.find((a) => a.name === name);
      if (pending) return pending.blob ? new Uint8Array(await pending.blob.arrayBuffer()) : bytesOfBase64(pending.data);
      const url = d.textFonts.some((f) => f.name === name) ? textFontUrl(owner, name) : embeddedFileUrl(owner, name);
      const r = await fetch(url, { cache: "no-store" });
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      return new Uint8Array(await r.arrayBuffer());
    };
    const picker = (window as unknown as { showDirectoryPicker?: DirectoryPicker }).showDirectoryPicker;
    if (!picker) {
      // no folder picker: a download per file
      for (const name of list) {
        let href: string;
        try {
          href = URL.createObjectURL(new Blob([(await bytesOf(name)()) as BlobPart]));
        } catch {
          setMessage(`Failed to open file '${name}'.`);
          continue;
        }
        const a = document.createElement("a");
        a.href = href;
        a.download = name;
        a.rel = "noopener";
        document.body.appendChild(a);
        a.click();
        a.remove();
        setTimeout(() => URL.revokeObjectURL(href), 60_000);
      }
      return;
    }
    let folder: ExportFolder;
    try {
      // "Select a directory to export files"
      folder = await picker({ mode: "readwrite", id: "embedded-files-export" });
    } catch {
      return; // the picker was cancelled
    }
    setBusy(true);
    const problems: string[] = [];
    try {
      const written = await exportInto(
        folder,
        list.map((name) => ({ name, bytes: bytesOf(name) })),
        (text) =>
          new Promise<{ answer: "overwrite" | "skip"; remember: boolean }>((resolve) =>
            ask({
              title: "Confirmation",
              text,
              yes: "Overwrite",
              no: "Skip",
              remember: true,
              onYes: (remember) => resolve({ answer: "overwrite", remember }),
              onNo: (remember) => resolve({ answer: "skip", remember }),
            })
          ).finally(() => setQuestion(null)),
        (m) => problems.push(m)
      );
      setMessage([...problems, written.length > 0 ? `Exported ${written.join(", ")}.` : ""].filter(Boolean).join("\n") || null);
    } finally {
      setBusy(false);
    }
  };

  // The page's Apply (the dialog's OK, `TransferDataFromWindow`): the copy as one verb.
  const apply = async () => {
    setMessage(null);
    let nested = false;
    if (asksNested(owner, listing, draft)) {
      nested = await new Promise<boolean>((resolve) => ask({ title: NESTED_TITLE, text: NESTED_QUESTION, yes: "Yes", no: "No", onYes: () => resolve(true), onNo: () => resolve(false) }));
      setQuestion(null);
    }
    const cmd = applyCmd(owner, listing, draft, nested);
    if (!cmd) return;
    setBusy(true);
    try {
      if (await run(cmd)) {
        setDraft(EMPTY_DRAFT);
        pendingChecksums.current.clear();
      }
      await reload();
    } finally {
      setBusy(false);
    }
  };

  const copyLink = (link: string) => {
    void navigator.clipboard?.writeText(link).then(
      () => setMessage(`Copied ${link}`),
      () => setMessage(link)
    );
  };

  // The actions `common.Embed.removeFile` and `extractFile` act on the panel that is showing.
  const removeRef = useRef(remove);
  const extractRef = useRef(exportFiles);
  removeRef.current = remove;
  extractRef.current = exportFiles;
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  useEffect(() => {
    if (hidden) return;
    const me: ActiveEmbeddedPanel = { owner, remove: () => removeRef.current([...selectedRef.current]), extract: () => void extractRef.current([...selectedRef.current]) };
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
                data-pending={r.pending ? "1" : undefined}
                aria-selected={selected.has(r.name)}
                className={selected.has(r.name) ? "selected" : undefined}
                style={{ cursor: "pointer", background: selected.has(r.name) ? "var(--chrome-selection, rgba(74,144,226,0.25))" : undefined, fontStyle: r.pending ? "italic" : undefined }}
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
                  {r.pending && (
                    <span className="bs-note" title="Taken in here; written to the list on OK">
                      {" "}
                      (not applied)
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
        <button disabled={busy || selected.size === 0} title="Remove an embedded file" onClick={() => remove([...selected])} data-testid="embedded-files-remove">
          Remove
        </button>
        <button disabled={busy || rows.length === 0} title="Extract an embedded file" onClick={() => void exportFiles(exportNames(rows))} data-testid="embedded-files-export">
          Export...
        </button>
        {fonts && (
          <label style={{ marginLeft: "auto" }} title="Store a copy of all fonts used">
            <input type="checkbox" checked={draftFontsOn(listing, draft)} disabled={busy || !listing} onChange={(e) => void clickFonts(e.target.checked)} data-testid="embedded-files-fonts" /> Embed fonts
          </label>
        )}
      </div>
      {question && (
        <div className="bs-toolbar" role="alertdialog" aria-label={question.title} style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }} data-testid="embedded-files-question">
          <span style={{ whiteSpace: "pre-line", marginRight: "auto" }}>{question.text}</span>
          {question.remember && (
            <label>
              <input type="checkbox" checked={rememberBox} onChange={(e) => setRememberBox(e.target.checked)} data-testid="embedded-files-question-remember" /> Do not show again
            </label>
          )}
          <button className="primary" onClick={() => question.onYes(rememberBox)}>
            {question.yes}
          </button>
          <button onClick={() => question.onNo(rememberBox)}>{question.no}</button>
          {question.cancel && <button onClick={question.cancel}>Cancel</button>}
        </div>
      )}
      {!inDialog && (
        <div className="bs-actions">
          <button className="primary" disabled={busy || !dirty} onClick={() => void apply()} data-testid="embedded-files-apply">
            Apply
          </button>
          <button
            disabled={busy || !dirty}
            onClick={() => {
              setDraft(EMPTY_DRAFT);
              pendingChecksums.current.clear();
              setMessage(null);
            }}
            data-testid="embedded-files-revert"
          >
            Revert
          </button>
          <span className="bs-msg" role="status">
            {dirty ? "Changes not applied yet." : ""}
          </span>
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
