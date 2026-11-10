// The Unfold from Bus menu (`C`, `BUS_UNFOLD_MENU::update` in eeschema/tools/sch_line_wire_bus_tool.cpp at 8303b2ad): each member of the bus, a member that is
// a bus itself a submenu of its own members, and -- UI-LEFT-2 SS18 -- the members of a differential pair (a name ending in `+`, `-`, `P` or `N`) gathered in a
// submenu named after the name without that last character:
//
//   std::unordered_map<wxString, ACTION_MENU*> diff_busses{};
//   ... if( member->Type() == CONNECTION_TYPE::BUS )
//           if( ( name.ends_with( '+' ) || ... ) && member->Members().size() > 0 )   // a suffixed bus: its submenu is shared by name
//               submenu = diff_busses[ name.substr( 0, len - 1 ) ] (made and appended to this menu the first time)
//           else
//               submenu = new ACTION_MENU, appended under PrintBusForUI( name )
//           for( sub_member : member->Members() )
//               if( suffixed( sub_name ) )  diff_busses[ sub_name minus its last character ] (made and appended to `submenu` the first time) gets the item
//               else                         submenu gets the item
//       else
//           this menu gets the item
//
// The map is one for the whole menu, so a pair met again under another member lands in the submenu made the first time. Every label is
// `SCH_CONNECTION::PrintBusForUI` of the name (the markup's braces dropped); the item's value is the member's full local name, which is what is unfolded.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).

/** One line of the menu: a member to unfold, or a submenu. */
export type UnfoldEntry = { kind: "item"; name: string; label: string } | { kind: "menu"; title: string; label: string; entries: UnfoldEntry[] };

/** A member of the bus as the server lists it (`member_tree`): its full local name and, for a member that is a bus itself, its own members. */
export interface UnfoldMember {
  name: string;
  members: readonly string[];
}

const isSuperSubOverbar = (c: string | undefined) => c === "_" || c === "^" || c === "~";

/**
 * `SCH_CONNECTION::PrintBusForUI`: a bus name as a menu shows it -- the prefix up to the group's `{`, then the members, with the braces of `_{..}`, `^{..}` and
 * `~{..}` markup and every `}` dropped and a backslash-escaped space shown as a space.
 */
export function printBusForUI(group: string): string {
  const g = Array.from(group);
  const n = g.length;
  let ret = "";
  let i = 0;
  // Parse prefix
  for (; i < n; ++i) {
    if (isSuperSubOverbar(g[i]) && i + 1 < n && g[i + 1] === "{") {
      ret += g[i];
      i++;
      continue;
    } else if (g[i] === "}") {
      continue;
    }
    if (g[i] === "\\" && i + 1 < n && g[i + 1] === " ") {
      ret += " ";
      i++;
      continue;
    }
    ret += g[i];
    if (g[i] === "{") break;
  }
  // Parse members
  i++; // '{' character
  for (; i < n; ++i) {
    if (isSuperSubOverbar(g[i]) && i + 1 < n && g[i + 1] === "{") {
      ret += g[i];
      i++;
      continue;
    } else if (g[i] === "}") {
      continue;
    }
    if (g[i] === "\\" && i + 1 < n && g[i + 1] === " ") {
      ret += " ";
      i++;
      continue;
    }
    ret += g[i];
    if (g[i] === "}") break;
  }
  return ret;
}

/** `name.ends_with( '+' ) || name.ends_with( '-' ) || name.ends_with( 'P' ) || name.ends_with( 'N' )` */
const suffixed = (name: string) => name.endsWith("+") || name.endsWith("-") || name.endsWith("P") || name.endsWith("N");

const item = (name: string): UnfoldEntry => ({ kind: "item", name, label: printBusForUI(name) });

/** `BUS_UNFOLD_MENU::update`'s entries for the members of a bus. */
export function busUnfoldMenu(members: readonly UnfoldMember[]): UnfoldEntry[] {
  const top: UnfoldEntry[] = [];
  const diffBusses = new Map<string, Extract<UnfoldEntry, { kind: "menu" }>>();
  const submenuIn = (into: UnfoldEntry[], title: string): Extract<UnfoldEntry, { kind: "menu" }> => {
    const m: Extract<UnfoldEntry, { kind: "menu" }> = { kind: "menu", title, label: printBusForUI(title), entries: [] };
    into.push(m);
    return m;
  };
  for (const member of members) {
    if (member.members.length === 0) {
      top.push(item(member.name));
      continue;
    }
    let submenu: Extract<UnfoldEntry, { kind: "menu" }>;
    if (suffixed(member.name)) {
      const key = member.name.slice(0, -1);
      submenu = diffBusses.get(key) ?? submenuIn(top, key);
      diffBusses.set(key, submenu);
    } else {
      submenu = submenuIn(top, member.name);
    }
    for (const sub of member.members) {
      if (suffixed(sub)) {
        const key = sub.slice(0, -1);
        const diff = diffBusses.get(key) ?? submenuIn(submenu.entries, key);
        diffBusses.set(key, diff);
        diff.entries.push(item(sub));
      } else {
        submenu.entries.push(item(sub));
      }
    }
  }
  return top;
}
