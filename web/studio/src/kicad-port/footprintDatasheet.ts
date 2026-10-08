// Show Datasheet in the Footprint Editor (`common.Control.showDatasheet`, `FOOTPRINT_EDITOR_CONTROL::ShowDatasheet`): which URL the footprint documents itself with.
// A port of `GetFootprintDocumentationURL` (pcbnew/generate_footprint_info.cpp, commit 8303b2ad): the footprint's Datasheet field when it has one, else the first
// http: or https: address written in its description -- "it is (or was) currently common practice to store a documentation link in the description" --
// read up to the first character a URI cannot hold (a blank, a non-ASCII character or a double quote) or the bracket that closes a parenthesis opened before it,
// and without a trailing ".", ",", ":" or ";".
import type { FootprintField } from "../api/types";

/** `FIELD_T::DATASHEET`'s name in a footprint's property list. */
const DATASHEET_FIELD = "Datasheet";

export function footprintDocumentationUrl(fields: readonly FootprintField[], description: string): string | null {
  const field = fields.find((f) => f.name.toLowerCase() === DATASHEET_FIELD.toLowerCase());
  const given = (field?.value ?? "").trim();
  if (given !== "") return given;

  let start = description.indexOf("http:");
  if (start < 0) start = description.indexOf("https:");
  if (start < 0) return null;

  let url = "";
  let nesting = 0;
  for (let i = start; i < description.length; i++) {
    const ch = description.charCodeAt(i);
    // Break on invalid URI characters
    if (ch <= 0x20 || ch >= 0x7f || ch === 0x22) break;
    // Check for nesting parentheses, e.g. (Body style from: https://this.url/part.pdf)
    if (ch === 0x28) nesting++;
    else if (ch === 0x29 && --nesting < 0) break;
    url += description[i];
  }

  // Trim trailing punctuation
  if (url !== "" && ".,:;".includes(url[url.length - 1]!)) url = url.slice(0, -1);
  return url === "" ? null : url;
}
