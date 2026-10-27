// Export IPC-2581's BOM columns, as `pcbnew/dialogs/dialog_export_2581.cpp` (DIALOG_EXPORT_2581) fills, guesses and keeps them (round 4's X4-4).
//
// - The four column controls are choices: the first item ("Generate unique", "Omit", "N/A") and then the field names of the board's
//   footprints (`init`: every `PCB_FIELD` of every footprint into a `std::set<wxString>`, so sorted and once each). A footprint always has
//   Reference, Value, Datasheet and Description (`FOOTPRINT::FOOTPRINT`'s `addField`s), then its user fields.
// - Choosing a Distributor P/N column fills Distributor from its name (`onDistPNChange`); choosing a Manufacturer P/N column picks a
//   "manufacturer" or "mfg" column for Manufacturer (`onMfgPNChange`).
// - On Export the choices are kept in the project (`TransferDataFromWindow`; here `set_ipc2581_bom`, stored in design.json) and read back at
//   the next opening (`TransferDataToWindow`), the BOM revision falling back to the root schematic's title block revision (`schRevision`).
//
// Pure: no React, no store (see tsconfig.test.json).

/** The stored columns (`eda_model::plot_params::Ipc2581Bom`, `PROJECT_FILE::m_IP2581Bom`); "" is the choice's first item. */
export interface Ipc2581Bom {
  id?: string;
  mpn?: string;
  mfg?: string;
  dist_pn?: string;
  dist?: string;
  bom_rev?: string;
}

/** What the dialog's BOM controls hold. `col_dist` is the Distributor text box ("N/A" while no Distributor P/N column is chosen). */
export interface Ipc2581Columns {
  bom_rev: string;
  col_id: string;
  col_mpn: string;
  col_mfg: string;
  col_dist_pn: string;
  col_dist: string;
}

/** `_( "N/A" )`: the Distributor text while it is disabled or not guessed. */
export const NOT_APPLICABLE = "N/A";

/** `FOOTPRINT::FOOTPRINT`: the fields every footprint has. */
const MANDATORY_FIELDS = ["Reference", "Value", "Datasheet", "Description"];

/** `DIALOG_EXPORT_2581::init`: the field names of the board's footprints, sorted as a `std::set<wxString>` sorts them (by code point), once each. */
export function ipc2581FieldOptions(footprints: Array<{ fields?: Array<{ name: string }> }>): string[] {
  const names = new Set<string>();
  for (const fp of footprints) {
    MANDATORY_FIELDS.forEach((n) => names.add(n));
    for (const f of fp.fields ?? []) names.add(f.name);
  }
  return [...names].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
}

/** `DIALOG_EXPORT_2581::onDistPNChange`'s guess: the distributor a part-number column's name names, else null. */
export function guessDistributor(column: string): string | null {
  const dist = column.toUpperCase();
  if (dist.includes("DIGIKEY")) return "Digi-Key";
  if (dist.includes("DIGI-KEY")) return "Digi-Key";
  if (dist.includes("MOUSER")) return "Mouser";
  if (dist.includes("NEWARK")) return "Newark";
  if (dist.includes("RS COMPONENTS")) return "RS Components";
  if (dist.includes("FARNELL")) return "Farnell";
  if (dist.includes("ARROW")) return "Arrow";
  if (dist.includes("AVNET")) return "Avnet";
  if (dist.includes("TME")) return "TME";
  if (dist.includes("LCSC")) return "LCSC";
  return null;
}

/** `onDistPNChange`: the Distributor text after Distributor P/N became `distPn` ("" is "Omit": disabled, "N/A"). A distributor the user
 *  has already typed (anything but "N/A") is kept. */
export function distributorAfter(distPn: string, current: string): string {
  if (distPn === "") return NOT_APPLICABLE;
  if (current !== NOT_APPLICABLE) return current;
  return guessDistributor(distPn) ?? current;
}

/** `onMfgPNChange`: the Manufacturer column after Manufacturer P/N became `mpn` ("" is "Omit": Manufacturer disabled). One already chosen is
 *  kept; else a column named "manufacturer", else "mfg" (`wxChoice::FindString`, which ignores case). */
export function manufacturerAfter(mpn: string, current: string, options: string[]): string {
  if (mpn === "" || current !== "") return current;
  const find = (s: string) => options.find((o) => o.toLowerCase() === s);
  return find("manufacturer") ?? find("mfg") ?? "";
}

/** `DIALOG_EXPORT_2581::TransferDataToWindow` (not a job): the controls from the stored columns, each kept only when the board still has a
 *  field of that name (`SetStringSelection` fails otherwise and the first item is chosen).
 *
 *  The Internal ID always starts at "Generate unique": KiCad's code sets the stored internal-id column on the Manufacturer P/N control
 *  (`if( !m_choiceMPN->SetStringSelection( internalIdCol ) )`), which the MPN column then overwrites, so `m_oemRef` keeps its first item. */
export function ipc2581DialogStart(stored: Ipc2581Bom | undefined, schRevision: string, options: string[]): Ipc2581Columns {
  const s = stored ?? {};
  const has = (c: string | undefined) => !!c && options.includes(c);
  const out: Ipc2581Columns = { bom_rev: s.bom_rev || schRevision, col_id: "", col_mpn: "", col_mfg: "", col_dist_pn: "", col_dist: NOT_APPLICABLE };
  if (has(s.mpn)) {
    out.col_mpn = s.mpn!;
    out.col_mfg = has(s.mfg) ? s.mfg! : "";
  }
  if (has(s.dist_pn)) {
    out.col_dist_pn = s.dist_pn!;
    // "The combo box selection can be fixed, so any value can be entered": the stored distributor as it is (the guess is only for a
    // project that kept no Distributor P/N column, which cannot reach here).
    out.col_dist = s.dist ?? "";
  }
  return out;
}

/** `TransferDataFromWindow` with `GetOEM`/`GetMPN`/`GetMfg`/`GetDistPN`/`GetDist`: what is kept, and what kicad-cli is given. Manufacturer counts
 *  only while a Manufacturer P/N column is chosen, Distributor only while a Distributor P/N column is and it is not "N/A". */
export function ipc2581Chosen(c: Ipc2581Columns): Required<Ipc2581Bom> {
  return {
    id: c.col_id,
    mpn: c.col_mpn,
    mfg: c.col_mpn ? c.col_mfg : "",
    dist_pn: c.col_dist_pn,
    dist: c.col_dist_pn && c.col_dist !== NOT_APPLICABLE ? c.col_dist : "",
    bom_rev: c.bom_rev,
  };
}
