// The simulator's probe cursors (KiCad 8303b2ad `common/gal/cursors.cpp`, `KICURSOR::VOLTAGE_PROBE` and `KICURSOR::CURRENT_PROBE`): the pointer
// SCH_EDITOR_CONTROL::SimProbe shows over the sheet -- the voltage probe over a wire or nothing, the current probe over a pin (its motion
// handler's `SetCursor`). The pictures are KiCad's 32 x 32 XPMs as the non-Windows build includes them (`voltage_probe_black.xpm`,
// `current_probe_black.xpm`: black, outlined in white), with their hot spots (`{ 1, 31 }`, `{ 4, 27 }`); the browser draws them as an SVG
// cursor. (KiCad's 64-pixel ones for HiDPI screens are not used.)

/** `resources/bitmaps_png/cursors/voltage_probe_black.xpm`: '.' white, '+' black, ' ' none. */
const VOLTAGE_PROBE = [
  "                                ",
  "                                ",
  "                                ",
  "                                ",
  "                                ",
  "                                ",
  "                                ",
  "                                ",
  "                     .....      ",
  "                    .+++++.     ",
  "                   .++...++.    ",
  "                 ..++.   .++.   ",
  "               ..+++.     .+.   ",
  "              .++++.      .+.   ",
  "             .+++++.      .++.  ",
  "            .++++++.       .++. ",
  "           .++++++.         .++.",
  "          .++++++.           .. ",
  "         .++++++.               ",
  "        .++++++.                ",
  "       .++++++.                 ",
  "      .++++++.                  ",
  "     .++++++.                   ",
  "    .++++++.                    ",
  "   .++++++.                     ",
  "   .+++++.                      ",
  "   .++++.                       ",
  "  .++...                        ",
  " .++.                           ",
  ".++.                            ",
  ".+.                             ",
  " .                              ",
];

/** `resources/bitmaps_png/cursors/current_probe_black.xpm`. */
const CURRENT_PROBE = [
  "                                ",
  "                                ",
  "                                ",
  "                                ",
  "                                ",
  "                                ",
  "                                ",
  "                                ",
  "                      ....      ",
  "                     .++..+.    ",
  "                    .++.  .+.   ",
  "                  ..++.    .+.  ",
  "                .++++.     .+.  ",
  "               .+++++.     .+.  ",
  "              .++++++.      .+. ",
  "       ..    .++++++.        .+.",
  "       ++.  .++++++.          . ",
  "       ++. .++++++.             ",
  "       ++..++++++.              ",
  "       ++.++++++.               ",
  "       +++++++.                 ",
  "       ++++++.                  ",
  "      .+++++.                   ",
  "   ...+++++.                    ",
  " .++++++++.                     ",
  " .+.. .+++.                     ",
  ".+.    .++.                     ",
  ".+.    .++.                     ",
  ".+.    .++.                     ",
  " .+.  .++.                      ",
  "  .++++++.                      ",
  "   .....                        ",
];

export type ProbeCursor = "voltage_probe" | "current_probe";

/** `cursors_defs`: the picture and the hot spot of each. */
const DEFS: Record<ProbeCursor, { rows: readonly string[]; hotspot: [number, number] }> = {
  voltage_probe: { rows: VOLTAGE_PROBE, hotspot: [1, 31] },
  current_probe: { rows: CURRENT_PROBE, hotspot: [4, 27] },
};

/** An XPM's pixels as an SVG: one rectangle per run of a colour along a row ('.' white, '+' black, anything else transparent). */
export function xpmToSvg(rows: readonly string[]): string {
  const width = Math.max(0, ...rows.map((r) => r.length));
  const rects: string[] = [];
  rows.forEach((row, y) => {
    let x = 0;
    while (x < row.length) {
      const c = row[x]!;
      let end = x + 1;
      while (end < row.length && row[end] === c) end++;
      if (c === "." || c === "+") rects.push(`<rect x="${x}" y="${y}" width="${end - x}" height="1" fill="${c === "." ? "#FFFFFF" : "#000000"}"/>`);
      x = end;
    }
  });
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${rows.length}" shape-rendering="crispEdges">${rects.join("")}</svg>`;
}

/** The CSS `cursor` of a probe: KiCad's picture at its hot spot, the crosshair where a browser cannot show it. */
export function probeCursorCss(kind: ProbeCursor): string {
  const d = DEFS[kind];
  return `url("data:image/svg+xml,${encodeURIComponent(xpmToSvg(d.rows))}") ${d.hotspot[0]} ${d.hotspot[1]}, crosshair`;
}
