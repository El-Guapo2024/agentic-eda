// A toolbar group button: KiCad's `ACTION_TOOLBAR_PALETTE` (common/tool/action_toolbar.cpp). The button shows and runs the group's
// current action -- the one last picked from the group, the first to begin with -- and a right-click (or the small arrow) opens the
// palette with every action of the group, where a pick runs that action and makes it the group's current one. Before this, a group
// could only ever run its first enabled member, so the others (the 45 degree crosshair, the other dimension kinds, ...) could not be
// reached from the toolbar.
import { useRef, useState } from "react";
import actionsData from "../kicad/actions.json";
import iconsData from "../kicad/icons.json";
import type { ActionsFile, IconsFile } from "../kicad/types";
import { effectiveHotkey } from "../actions/hotkeys";
import { useActionRunner } from "../actions/useActionRunner";
import { useColorScheme } from "../hooks/useColorScheme";
import { ContextMenu, type MenuEntry } from "./canvas/ContextMenu";

const actionsByName = new Map((actionsData as ActionsFile).actions.map((a) => [a.name, a]));
const iconsFile = iconsData as IconsFile;

/** The member each group last ran, by group (its actions joined): it outlives the button, which is re-created on every tab switch. */
const lastPicked = new Map<string, string>();

export function ToolbarGroup({ label, icon, items }: { label: string; icon: string | null; items: string[] }) {
  const { run, isEnabled } = useActionRunner();
  const scheme = useColorScheme();
  const key = items.join("|");
  const [, setTick] = useState(0);
  const [palette, setPalette] = useState<{ x: number; y: number } | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  // The current member, as long as it is still enabled; otherwise the first enabled one.
  const picked = lastPicked.get(key);
  const current = picked && items.includes(picked) && isEnabled(picked) ? picked : items.find((a) => isEnabled(a));
  const currentAction = current ? actionsByName.get(current) : undefined;
  const iconFile = (currentAction?.icon ? iconsFile.icons[currentAction.icon] : null) ?? (icon ? iconsFile.icons[icon] : null);
  const title = current ? [currentAction?.label ?? current, current ? effectiveHotkey(currentAction!).hotkey : null].filter(Boolean).join(" — ") : `${label} (not ported yet)`;

  const entries: MenuEntry[] = items.map((name) => {
    const a = actionsByName.get(name);
    const hk = a ? effectiveHotkey(a).hotkey : null;
    return {
      label: `${a?.label ?? name}${hk ? `   ${hk}` : ""}`,
      disabled: !isEnabled(name),
      onSelect: () => {
        lastPicked.set(key, name);
        setTick((t) => t + 1);
        run(name);
      },
    };
  });

  const openPalette = () => {
    const r = ref.current?.getBoundingClientRect();
    if (r) setPalette({ x: r.left, y: r.bottom });
  };

  return (
    <div ref={ref} style={{ display: "flex", position: "relative" }} onContextMenu={(e) => { e.preventDefault(); openPalette(); }}>
      <button className="toolbar-button" disabled={!current} title={title} onClick={() => current && run(current)}>
        {iconFile ? <img src={`/icons/${scheme}/${iconFile}`} width={24} height={24} alt="" draggable={false} /> : <span className="icon-placeholder" aria-hidden />}
      </button>
      {items.length > 1 && (
        <button
          className="toolbar-button"
          aria-label={`${label}: more actions`}
          title={`${label}: more actions`}
          onClick={openPalette}
          style={{ width: 10, padding: 0, marginLeft: -4, fontSize: 8, alignSelf: "flex-end", height: 14, border: "none" }}
        >
          {"▾"}
        </button>
      )}
      {palette && <ContextMenu x={palette.x} y={palette.y} entries={entries} onClose={() => setPalette(null)} />}
    </div>
  );
}
