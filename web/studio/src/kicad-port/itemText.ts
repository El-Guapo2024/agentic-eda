// Pure pieces of two small "text" actions.
//
// Copy as Text -- EDIT_TOOL::copyToClipboardAsText (pcbnew/tools/edit_tool.cpp)
// and SCH_EDITOR_CONTROL::CopyAsText + GetSelectedItemsAsText
// (eeschema/tools/sch_tool_utils.cpp): every selected item's text, each
// trimmed (`Trim(false).Trim(true)`), empties dropped, joined with '\n'.
// Items with no text representation contribute "" and are skipped.
//
// Show Datasheet -- SCH_INSPECTION_TOOL::ShowDatasheet
// (eeschema/tools/sch_inspection_tool.cpp): `datasheet.IsEmpty() ||
// datasheet == "~"` -> "No datasheet defined."; otherwise
// GetAssociatedDocument() launches it. Here only an absolute http(s)/file
// URL can be launched from a browser; anything else (a bare file name that
// KiCad would resolve through the project search stack) cannot.

export function selectionAsText(texts: ReadonlyArray<string | null | undefined>): string {
  return texts
    .map((t) => (t ?? "").trim())
    .filter((t) => t !== "")
    .join("\n");
}

export type DatasheetTarget = { kind: "none" } | { kind: "url"; url: string } | { kind: "unresolvable"; text: string };

export function datasheetTarget(field: string | null | undefined): DatasheetTarget {
  const text = (field ?? "").trim();
  if (text === "" || text === "~") return { kind: "none" };
  if (/^(https?|file):\/\//i.test(text)) return { kind: "url", url: text };
  // GetAssociatedDocument also accepts "www.x.com/..." style hosts.
  if (/^www\./i.test(text)) return { kind: "url", url: `https://${text}` };
  return { kind: "unresolvable", text };
}
