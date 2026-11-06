// The selection clarification menu (`SELECTION_TOOL::doSelectionMenu`, common/tool/selection_tool.cpp at 8303b2ad): the rows a click on several items
// offers, numbered, and the key each answers to. UI-LEFT P22. Pure; components/canvas/Canvas.tsx (the board) and components/CommonToolHost.tsx (the
// other editors) show it with components/canvas/ContextMenu.tsx, which takes the keys.
//
//   for( int i = 0; i < limit; ++i )                                    limit = std::min( 100, aCollector->GetCount() )
//       if( i < 9 )  menuText = "&%d  %s\t%d" ( i + 1, description, i + 1 )   ("%s\t%d" on macOS: no number before the text)
//       else         menuText = description
//   menu.AppendSeparator();
//   menu.Add( _( "Select &All\tA" ), limit + 1 );
//   if( !showMoreChoices && aCollector->HasAdditionalItems() ) menu.Add( _( "Show &More Choices...\tM" ), limit + 2 );
//
// The `&` marks the mnemonic (the key the open menu takes for the row) and the text after `\t` is the accelerator shown at the right: here they are the
// same key, so a row is chosen by pressing its number, "A" or "M". Pointing at a row brightens its item, at "Select All" every item
// (`TA_CHOICE_MENU_UPDATE` -> `highlight( current, BRIGHTENED, &highlightGroup )`).

/** `std::min( 100, aCollector->GetCount() )`. */
export const MENU_LIMIT = 100;

export type ClarificationRow =
  | { kind: "item"; index: number; label: string; accel?: string }
  | { kind: "all"; label: string; accel: string }
  | { kind: "more"; label: string; accel: string };

/** The rows for `descriptions` (the collector's items in order), "Select All", and "Show More Choices..." when the guess left items out (`HasAdditionalItems()`). */
export function clarificationRows(descriptions: readonly string[], opts: { mac: boolean; more: boolean }): ClarificationRow[] {
  const limit = Math.min(MENU_LIMIT, descriptions.length);
  const rows: ClarificationRow[] = [];
  for (let i = 0; i < limit; i++) {
    const desc = descriptions[i]!;
    if (i < 9) rows.push({ kind: "item", index: i, label: opts.mac ? desc : `${i + 1}  ${desc}`, accel: `${i + 1}` });
    else rows.push({ kind: "item", index: i, label: desc });
  }
  rows.push({ kind: "all", label: "Select All", accel: "A" });
  if (opts.more) rows.push({ kind: "more", label: "Show More Choices...", accel: "M" });
  return rows;
}

/** The entry a key chooses while the menu is open: the first whose accelerator is that key (letters in either case); -1 for none. */
export function accelIndex(entries: readonly { accel?: string; disabled?: boolean; separator?: boolean }[], key: string): number {
  if (key.length !== 1) return -1;
  const k = key.toUpperCase();
  return entries.findIndex((e) => !e.separator && !e.disabled && e.accel !== undefined && e.accel.toUpperCase() === k);
}
