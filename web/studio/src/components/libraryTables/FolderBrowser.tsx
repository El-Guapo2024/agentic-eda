// The stand-in for the native file dialog the browse buttons of KiCad's library table dialogs open (`wxFileDialog`, `wxDirDialog`): a page cannot see the machine's
// folders, the studio's server can. It lists the folders of a path and the files the caller asked for -- a `.kicad_sym` file, a `.pretty` footprint folder, a
// `.kicad_blocks` design block folder, a library table, or a folder -- and hands back the absolute path of the one chosen (`GET /api/fs/list`,
// `crates/cli/src/library_tables.rs` `browse`).
import { useEffect, useState } from "react";
import { browseFolder, type FolderEntry, type FolderListing } from "../../api/libraryTablesClient";

export type BrowseWhat = "symbol" | "footprint" | "design_block" | "table" | "folder";

export interface FolderBrowserProps {
  title: string;
  what: BrowseWhat;
  /** Where to start: a path (variables allowed), or empty for the board's folder. */
  start: string;
  onPick: (path: string) => void;
  onCancel: () => void;
}

function join(folder: string, name: string): string {
  return folder.endsWith("/") ? folder + name : `${folder}/${name}`;
}

const HINT: Record<BrowseWhat, string> = {
  symbol: "Choose a .kicad_sym file.",
  footprint: "Choose a footprint library: a .pretty folder (any folder works; its whole name becomes the nickname).",
  design_block: "Choose a design block library: a .kicad_blocks folder (any folder works; its whole name becomes the nickname).",
  table: "Choose a library table file (sym-lib-table, fp-lib-table or design-block-lib-table).",
  folder: "Choose the folder.",
};

/** Whether picking this entry makes sense for what is being chosen. */
function pickable(what: BrowseWhat, e: FolderEntry): boolean {
  if (what === "folder") return false;
  if (what === "footprint" || what === "design_block") return e.dir;
  return !e.dir;
}

export function FolderBrowser({ title, what, start, onPick, onCancel }: FolderBrowserProps) {
  const [at, setAt] = useState(start);
  const [typed, setTyped] = useState(start);
  const [listing, setListing] = useState<FolderListing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setError(null);
    browseFolder(at, what)
      .then((l) => {
        if (cancelled) return;
        setListing(l);
        setTyped(l.path);
        setSelected(null);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [at, what]);

  const entry = listing?.entries.find((e) => e.name === selected) ?? null;
  const chosen = what === "folder" ? (listing?.path ?? null) : entry && pickable(what, entry) && listing ? join(listing.path, entry.name) : null;

  const open = (e: FolderEntry) => {
    if (!listing) return;
    if (e.dir && !((what === "footprint" || what === "design_block") && e.library)) setAt(join(listing.path, e.name));
    else if (pickable(what, e)) onPick(join(listing.path, e.name));
  };

  return (
    <div className="dialog-backdrop lt-browser-backdrop" onClick={onCancel}>
      <div className="dialog lt-browser" onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Escape" && onCancel()} role="dialog" aria-label={title}>
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body lt-browser-body">
          <div className="lt-browser-bar">
            <button title="Up one folder" aria-label="Up one folder" disabled={!listing?.parent} onClick={() => listing?.parent && setAt(listing.parent)}>
              {"↑"}
            </button>
            <input
              aria-label="Folder"
              value={typed}
              spellCheck={false}
              onChange={(e) => setTyped(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.stopPropagation();
                  setAt(typed);
                }
              }}
            />
            <button title="Show this folder" onClick={() => setAt(typed)}>
              Go
            </button>
          </div>
          <div className="lt-browser-quick">
            {listing?.home && (
              <button className="lt-link" onClick={() => setAt(listing.home as string)}>
                Home
              </button>
            )}
            {listing && (
              <button className="lt-link" onClick={() => setAt(listing.project)}>
                Board folder
              </button>
            )}
          </div>
          <div className="lt-browser-list" role="listbox" aria-label="Folders and files">
            {error && <div className="lt-error">{error}</div>}
            {listing?.entries.length === 0 && !error && <div className="lt-dim">Nothing to choose in this folder.</div>}
            {listing?.entries.map((e) => (
              <div
                key={e.name}
                role="option"
                aria-selected={e.name === selected}
                data-name={e.name}
                className={`lt-browser-item${e.name === selected ? " selected" : ""}`}
                onClick={() => setSelected(e.name)}
                onDoubleClick={() => open(e)}
              >
                <span className="lt-browser-icon">{e.library ? "▤" : e.dir ? "▸" : "·"}</span>
                <span>{e.name}</span>
              </div>
            ))}
            {listing?.truncated && <div className="lt-dim">Only the first entries are listed.</div>}
          </div>
          <div className="lt-dim lt-browser-hint">{HINT[what]}</div>
        </div>
        <div className="dialog-footer">
          <button onClick={onCancel}>Cancel</button>
          <button className="primary" disabled={!chosen} onClick={() => chosen && onPick(chosen)}>
            {what === "folder" ? "Select This Folder" : "Select"}
          </button>
        </div>
      </div>
    </div>
  );
}
