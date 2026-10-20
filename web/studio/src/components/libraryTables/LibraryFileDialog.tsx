// New Library and Add Library of the Symbol Editor, the Footprint Editor and the Design Blocks panel: the file dialog of `SYMBOL_EDIT_FRAME::AddLibraryFile`
// (eeschema/symbol_editor/symbol_edit_frame.cpp), `PCB_BASE_EDIT_FRAME::CreateNewLibrary` / `AddLibrary` (pcbnew/footprint_libraries_utils.cpp) and
// `DESIGN_BLOCK_PANE::createNewDesignBlockLibrary` / `AddDesignBlockLibrary` (common/widgets/design_block_pane.cpp).
//
// New Library makes an empty `.kicad_sym` file (symbols), `.pretty` folder (footprints) or `.kicad_blocks` folder (design blocks) and a row for it in the library table; Add Library puts a library that exists in
// the table. KiCad's file dialog has a "Add to global / project library table" choice; the global tables are KiCad's and are not written here, so the row goes into
// the board's own table (`sym-lib-table`, `fp-lib-table`, `design-block-lib-table`), which is said in the dialog. The path is written as a table writes one (`NormalizePath`: under a variable
// or `${KIPRJMOD}` where one holds it), the nickname is the file's name (`LIB_ID::FixIllegalChars`), and a nickname a library already has is refused ("Library '%s'
// already exists.").
import { useEffect, useState } from "react";
import { addLibrary, fetchLibraryTables, type LibraryTablesView } from "../../api/libraryTablesClient";
import { baseName, libraryExtension, nicknameForFile, tableFileName, type TableKind } from "../../kicad-port/libraryTable";
import { closeLibraryFile, useLibraryTableDialogs } from "../../state/libraryTableDialogs";
import { selectDesignBlockLibrary } from "../../state/designBlocks";
import { selectLibraries, setLibraryOpen } from "../../state/libraryTree";
import { useStudioDispatch } from "../../state/store";
import { FolderBrowser, type BrowseWhat } from "./FolderBrowser";
import "../../styles/libraryTables.css";

export function LibraryFileDialog() {
  const req = useLibraryTableDialogs().library;
  return req ? <FileBody key={`${req.kind}:${req.create}`} kind={req.kind} create={req.create} /> : null;
}

function FileBody({ kind, create }: { kind: TableKind; create: boolean }) {
  const dispatch = useStudioDispatch();
  const [view, setView] = useState<LibraryTablesView | null>(null);
  const [folder, setFolder] = useState("");
  const [name, setName] = useState("");
  const [path, setPath] = useState("");
  const [nickname, setNickname] = useState("");
  const [nicknameTyped, setNicknameTyped] = useState(false);
  const [descr, setDescr] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [browse, setBrowse] = useState<BrowseWhat | null>(null);

  const noun = kind === "symbol" ? "Symbol" : kind === "footprint" ? "Footprint" : "Design Block";
  const title = `${create ? "New" : "Add"} ${noun} Library`;
  const ext = libraryExtension(kind);

  useEffect(() => {
    let cancelled = false;
    fetchLibraryTables()
      .then((v) => {
        if (cancelled) return;
        setView(v);
        setFolder(v.project_dir);
        setName(v.tables[kind].new_library_name);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [kind]);

  const close = () => closeLibraryFile();
  const target = create ? `${folder.replace(/\/+$/, "")}/${name}${name.endsWith(ext) ? "" : ext}` : path;

  const setThePath = (p: string) => {
    setError(null);
    setPath(p);
    if (!nicknameTyped) setNickname(p.trim() === "" ? "" : nicknameForFile(kind, baseName(p)));
  };

  const ok = async () => {
    setError(null);
    if (create && name.trim() === "") return setError("Give the new library a name.");
    if (!create && path.trim() === "") return setError("Choose a library to add.");
    setBusy(true);
    try {
      const answer = await addLibrary({ kind, path: target, create, nickname: create ? undefined : nickname.trim() || undefined, descr: descr.trim() || undefined });
      if ("ok" in answer && answer.ok) {
        const lib = answer.added.nickname;
        dispatch({ type: "TOAST", message: `${answer.added.created ? "Created" : "Added"} library '${lib}' in the project's ${tableFileName(kind)}.`, kind: "info" });
        // `FocusOnLibID`: the new library is the one selected in the tree (`AddDesignBlockLibrary`: `SelectLibId( LIB_ID( libName, "" ) )`)
        if (kind === "design_block") {
          selectDesignBlockLibrary(lib);
        } else {
          setLibraryOpen(kind, lib, true);
          selectLibraries(kind, [lib]);
        }
        close();
      } else if ("error" in answer) {
        setError(answer.error);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog lt-small" onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Escape" && browse === null && close()} role="dialog" aria-label={title}>
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body">
          <div className="lt-form">
            {create ? (
              <>
                <label htmlFor="lt-folder">Folder:</label>
                <input id="lt-folder" type="text" value={folder} spellCheck={false} onChange={(e) => setFolder(e.target.value)} />
                <button onClick={() => setBrowse("folder")}>Browse...</button>
                <label htmlFor="lt-name">Name:</label>
                <input id="lt-name" type="text" autoFocus value={name} spellCheck={false} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void ok()} />
                <span />
              </>
            ) : (
              <>
                <label htmlFor="lt-path">{kind === "symbol" ? "File:" : "Folder:"}</label>
                <input id="lt-path" type="text" autoFocus value={path} spellCheck={false} placeholder={kind === "symbol" ? `/path/to/Library${ext}` : `/path/to/Library${ext}`} onChange={(e) => setThePath(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void ok()} />
                <button onClick={() => setBrowse(kind)}>Browse...</button>
                <label htmlFor="lt-nick">Nickname:</label>
                <input
                  id="lt-nick"
                  type="text"
                  value={nickname}
                  spellCheck={false}
                  onChange={(e) => {
                    setNicknameTyped(true);
                    setNickname(e.target.value);
                    setError(null);
                  }}
                />
                <span />
              </>
            )}
            <label htmlFor="lt-descr">Description:</label>
            <input id="lt-descr" type="text" value={descr} spellCheck={false} onChange={(e) => setDescr(e.target.value)} />
            <span />
          </div>
          {create && <div className="lt-note" style={{ marginTop: 8 }}>{`Makes ${target}${kind === "symbol" ? ", an empty symbol library," : kind === "footprint" ? ", an empty footprint library," : ", an empty design block library,"} and adds it to the board's library table.`}</div>}
          <div className="lt-note" style={{ marginTop: 6 }}>
            {`The library goes in the project's ${tableFileName(kind)}${view ? ` (${view.tables[kind].project.path})` : ""}. KiCad's global table is read only here.`}
          </div>
          {error && <div className="lt-error" role="alert" style={{ marginTop: 8 }}>{error}</div>}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={busy} onClick={() => void ok()}>
            {create ? "Create" : "Add"}
          </button>
        </div>
      </div>
      {browse && (
        <div onClick={(e) => e.stopPropagation()}>
          <FolderBrowser
            title={browse === "folder" ? "Select Folder" : `Select ${noun} Library`}
            what={browse}
            start={browse === "folder" ? folder : path}
            onPick={(p) => {
              setBrowse(null);
              if (browse === "folder") setFolder(p);
              else setThePath(p);
            }}
            onCancel={() => setBrowse(null)}
          />
        </div>
      )}
    </div>
  );
}
