// Renders one toolbar from src/kicad/toolbars.json (main/options/drawing/
// auxiliary -- see App.tsx for where each is docked). Data-driven, same
// as MenuBar: empty until tools/extract-toolbars.js has real output.
import React from "react";
import toolbarsData from "../kicad/toolbars.json";
import actionsData from "../kicad/actions.json";
import iconsData from "../kicad/icons.json";
import type { ToolbarsFile, ToolbarId, ActionsFile, IconsFile, ToolbarItem } from "../kicad/types";
import { useActionRunner } from "../actions/useActionRunner";
import { useColorScheme } from "../hooks/useColorScheme";

const toolbarsFile = toolbarsData as ToolbarsFile;
const actionsFile = actionsData as ActionsFile;
const iconsFile = iconsData as IconsFile;
const actionsByName = new Map(actionsFile.actions.map((a) => [a.name, a]));

function ActionIcon({ iconName }: { iconName: string | null }) {
  const scheme = useColorScheme();
  const file = iconName ? iconsFile.icons[iconName] : null;
  if (!file) return <span className="icon-placeholder" aria-hidden />;
  return <img src={`/icons/${scheme}/${file}`} width={16} height={16} alt="" draggable={false} />;
}

function ToolbarItemView({ item }: { item: ToolbarItem }) {
  const { run, isEnabled } = useActionRunner();

  if (item.type === "separator") return <div className="toolbar-separator" role="separator" />;

  if (item.type === "control") {
    return (
      <div className="toolbar-control" title={item.label ?? item.control}>
        <span className="icon-placeholder" aria-hidden />
        <span>{item.label ?? item.control}</span>
      </div>
    );
  }

  if (item.type === "group") {
    const enabled = item.items.some((a) => isEnabled(a));
    return (
      <button className="toolbar-button" disabled={!enabled} title={enabled ? item.label : `${item.label} (not ported yet)`}>
        <ActionIcon iconName={item.icon} />
      </button>
    );
  }

  const action = actionsByName.get(item.action);
  const enabled = isEnabled(item.action);
  const label = action?.label ?? item.action;
  const tooltip = enabled ? [label, action?.hotkey].filter(Boolean).join(" — ") : `${label} (not ported yet)`;
  return (
    <button className="toolbar-button" disabled={!enabled} title={tooltip} onClick={() => run(item.action)}>
      <ActionIcon iconName={action?.icon ?? null} />
    </button>
  );
}

export function Toolbar({ id }: { id: ToolbarId }) {
  const config = toolbarsFile.toolbars.find((t) => t.id === id);
  const orientation = config?.orientation ?? (id === "options" || id === "drawing" ? "vertical" : "horizontal");

  if (!config || config.items.length === 0) {
    return (
      <div className={`toolbar${orientation === "vertical" ? " vertical" : ""}`} data-toolbar={id}>
        {id === "main" && <span style={{ color: "var(--chrome-text-dim)", fontStyle: "italic", padding: "0 6px" }}>KiCad toolbar data not extracted yet (tools/extract-toolbars.js)</span>}
      </div>
    );
  }

  return (
    <div className={`toolbar${orientation === "vertical" ? " vertical" : ""}`} data-toolbar={id}>
      {config.items.map((item, i) => (
        <ToolbarItemView key={i} item={item} />
      ))}
    </div>
  );
}
