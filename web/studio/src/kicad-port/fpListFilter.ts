// The filter box of KiCad's `EDA_LIST_DIALOG` (common/dialogs/eda_list_dialog.cpp), as the Footprint Editor's Load footprint from current PCB list uses it.
/** `EDA_LIST_DIALOG::textChangeInFilterBox`: `wxString::Matches( "*" + text.MakeLower() + "*" )` on the lower-cased row (`*` any run, `?` one character). */
export function listFilter(text: string): (row: string) => boolean {
  const pattern = `*${text.toLowerCase()}*`;
  const body = pattern.replace(/[.+^$(){}|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".");
  const re = new RegExp("^" + body + "$", "s");
  return (row) => re.test(row.toLowerCase());
}
