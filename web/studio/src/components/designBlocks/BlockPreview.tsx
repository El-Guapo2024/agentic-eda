// The preview of the selected design block (`SCH_DESIGN_BLOCK_PREVIEW_WIDGET`, `PCB_DESIGN_BLOCK_PREVIEW_WIDGET`): the block's schematic, or its board in the board editor, drawn by
// kicad-cli (`GET /api/design_blocks/preview`; the server runs `sch export svg` / `pcb export svg` in its one kicad-cli lane and keeps the answer until the file changes) and
// fitted to the room with a little whitespace round it (`fitOnDrawArea`: the scale is divided by 1.2). A block with no such design says so (`SetStatusText`).
import { useEffect, useRef, useState } from "react";
import { fetchBlockPreview } from "../../api/designBlockClient";
import { cropViewBox, fluidSvg, sanitizeSvg } from "../../kicad-port/svgPreview";

export function BlockPreview({ lib, name, what, has }: { lib: string; name: string; what: "sch" | "pcb"; has: boolean }) {
  const [svg, setSvg] = useState<string | null>(null);
  const [status, setStatus] = useState<string>("");
  const holder = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setSvg(null);
    if (!has) {
      setStatus(what === "sch" ? "Design block has no schematic." : "Design block has no layout.");
      return;
    }
    setStatus("Loading preview...");
    const ctl = new AbortController();
    // a short pause: moving down the tree with the arrow keys does not start a run for every block it passes
    const timer = window.setTimeout(() => {
      fetchBlockPreview(lib, name, what, ctl.signal).then(
        (r) => {
          if (ctl.signal.aborted) return;
          if (r.ok) {
            setSvg(fluidSvg(sanitizeSvg(r.svg)));
            setStatus("");
          } else {
            setStatus(r.message);
          }
        },
        (e: unknown) => {
          if (!ctl.signal.aborted && !(e instanceof DOMException && e.name === "AbortError")) setStatus(e instanceof Error ? e.message : String(e));
        }
      );
    }, 150);
    return () => {
      window.clearTimeout(timer);
      ctl.abort();
    };
  }, [lib, name, what, has]);

  // fit the picture to its drawing: kicad-cli draws a schematic on its whole page
  useEffect(() => {
    const root = holder.current?.querySelector("svg");
    if (!root || !svg) return;
    try {
      const b = (root as SVGGraphicsElement).getBBox();
      const box = cropViewBox({ x: b.x, y: b.y, width: b.width, height: b.height });
      if (box) root.setAttribute("viewBox", box);
    } catch {
      // not laid out (a hidden pane): the whole page stays
    }
    root.setAttribute("preserveAspectRatio", "xMidYMid meet");
  }, [svg]);

  return (
    <div className="db-preview" data-testid="db-preview" aria-label={`Preview of ${lib}:${name}`}>
      {svg ? <div ref={holder} className="db-preview-svg" dangerouslySetInnerHTML={{ __html: svg }} /> : <div className="db-preview-status">{status}</div>}
    </div>
  );
}
