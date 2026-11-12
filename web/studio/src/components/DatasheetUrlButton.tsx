// The button of a Datasheet cell: KiCad 25ec2335's `TEXT_BUTTON_URL` (common/widgets/grid_text_button_helpers.cpp, the fields grids' `GRID_CELL_URL_EDITOR`) and
// `PG_URL_EDITOR` (common/properties/pg_editors.cpp, the Properties panel's Datasheet row). With a value it launches the document (`GetAssociatedDocument`): a
// web or file URL in a tab of its own, a `kicad-embed://` link out of the first list of the files stack that has it (`aFilesStack`; the fields grids pass the
// dialog's stack, the Properties panel none -- `GetAssociatedDocument( m_frame, filename, &m_frame->Prj() )` -- so there KiCad opens nothing and says nothing,
// "No EMBEDDED_FILES object provided for kicad_embed URI" going to its trace). Its bitmap is the globe (`BITMAPS::www`) then, the folder
// (`BITMAPS::small_folder`) when the value is empty or "~".
//
// Differs: with no value KiCad opens the machine's file dialog and keeps `file://<path>` (or, with its "Embed file" box, embeds the file); a page gets no path
// from the browser's chooser, so the empty cell's button is disabled -- a file is embedded with the Embedded Files tab and named by its `kicad-embed://` link.
import { datasheetTarget } from "../kicad-port/itemText";
import { openEmbeddedDocument } from "../api/embeddedClient";
import type { EmbeddedOwner } from "../api/embeddedTypes";

export function DatasheetUrlButton({ value, stack, label = "Datasheet" }: { value: string | null | undefined; stack: readonly EmbeddedOwner[]; label?: string }) {
  const target = datasheetTarget(value);
  const empty = target.kind === "none";
  const open = () => {
    if (target.kind === "url") window.open(target.url, "_blank", "noopener,noreferrer");
    else if (target.kind === "embedded" && stack.length > 0) void openEmbeddedDocument(stack, target.link);
  };
  return (
    <button
      type="button"
      className="datasheet-url"
      aria-label={`Open ${label}`}
      title={empty ? "A file is chosen with the browser's chooser, which gives no path: embed it on the Embedded Files tab and name it by its kicad-embed:// link" : `Open ${value}`}
      disabled={empty}
      data-icon={empty ? "small_folder" : "www"}
      onClick={open}
      style={{ padding: "0 6px", flex: "none" }}
    >
      {empty ? "…" : "www"}
    </button>
  );
}
