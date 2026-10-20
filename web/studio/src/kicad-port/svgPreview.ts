// The preview of a design block is kicad-cli's SVG of the block's file (`sch export svg`, `pcb export svg`; the server runs it, crates/cli/src/design_block_api.rs). The text comes from
// a file the person's library holds, so it is made safe before it goes into the page -- no script, no foreign object, no event handler, no `javascript:` link -- and the picture is
// made fluid (a fixed size in millimetres would not fit the pane). Pure; the page then crops the picture to its drawing (`SVGGraphicsElement.getBBox`).

/** The SVG text with everything that could run or fetch taken out. */
export function sanitizeSvg(svg: string): string {
  return svg
    .replace(/<\?xml[\s\S]*?\?>/gi, "")
    .replace(/<!DOCTYPE[\s\S]*?>/gi, "")
    .replace(/<!--[\s\S]*?-->/g, "")
    .replace(/<script[\s\S]*?<\/script\s*>/gi, "")
    .replace(/<script\b[^>]*\/>/gi, "")
    .replace(/<foreignObject[\s\S]*?<\/foreignObject\s*>/gi, "")
    .replace(/<(iframe|object|embed|image|use|animate|set)\b[^>]*>(?:[\s\S]*?<\/\1\s*>)?/gi, "")
    .replace(/\s(?:on[a-z]+)\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)/gi, "")
    .replace(/\s(?:xlink:)?href\s*=\s*(?:"\s*javascript:[^"]*"|'\s*javascript:[^']*')/gi, "")
    .trim();
}

/** The root `<svg>` with its `width` and `height` (millimetres of the page) replaced by 100%, so the picture scales by its `viewBox` into whatever room it is given. */
export function fluidSvg(svg: string): string {
  const open = svg.search(/<svg\b/i);
  if (open < 0) return svg;
  const end = svg.indexOf(">", open);
  if (end < 0) return svg;
  const tag = svg.slice(open, end + 1).replace(/\swidth\s*=\s*"[^"]*"/i, ' width="100%"').replace(/\sheight\s*=\s*"[^"]*"/i, ' height="100%"');
  return svg.slice(0, open) + tag + svg.slice(end + 1);
}

/** A `viewBox` that holds a drawing's box with a margin of `fraction` of its larger side round it (`SCH_DESIGN_BLOCK_PREVIEW_WIDGET::fitOnDrawArea` leaves "a bit of whitespace": the scale is divided by 1.2). */
export function cropViewBox(box: { x: number; y: number; width: number; height: number }, fraction = 0.1): string | null {
  if (!(box.width > 0 && box.height > 0) || !Number.isFinite(box.x + box.y + box.width + box.height)) return null;
  const pad = Math.max(box.width, box.height) * fraction;
  return `${box.x - pad} ${box.y - pad} ${box.width + 2 * pad} ${box.height + 2 * pad}`;
}
