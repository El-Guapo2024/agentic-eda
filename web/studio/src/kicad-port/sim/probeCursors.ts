// The simulator's probe and tune cursors (KiCad 8303b2ad `common/gal/cursors.cpp`, `KICURSOR::VOLTAGE_PROBE`, `KICURSOR::CURRENT_PROBE` and
// `KICURSOR::TUNE`): the pointer SCH_EDITOR_CONTROL::SimProbe shows over the sheet -- the voltage probe over a wire or nothing, the current probe over a pin
// (its motion handler's `SetCursor`) -- and SimTune's picker (`picker->SetCursor( KICURSOR::TUNE )`). The pictures are KiCad's XPMs as the non-Windows build
// includes them (`voltage_probe_black.xpm`, `current_probe_black.xpm`, `cursor_tune_black.xpm` and their 64-pixel HiDPI pairs), with `cursors_defs`' hot
// spots; the browser draws them as SVG cursors. KiCad's wxWidgets 3.3 build hands both sizes over as one `wxCursorBundle` (`storeGetBundle`), which the
// system draws at the screen's scale; here the CSS `image-set()` of the two (UI-LEFT-3 M3-2), the 32-pixel one where a browser takes no image set.

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

/** `resources/bitmaps_png/cursors/voltage_probe_black64.xpm` ('.' white, '+' black). */
const VOLTAGE_PROBE_64 = [
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                             ....               ",
  "                                          ...++++...            ",
  "                                         .++++++++++.           ",
  "                                        .++++++++++++.          ",
  "                                       .+++++....+++++.         ",
  "                                      .+++++.    .+++++.        ",
  "                                     .+++++.      .+++++.       ",
  "                                  ...+++++.        .++++.       ",
  "                                 .+++++++.          .+++.       ",
  "                              ...+++++++.           .+++.       ",
  "                             .+++++++++.             .++.       ",
  "                            .+++++++++.              .+++.      ",
  "                           .++++++++++.              .++++.     ",
  "                          .+++++++++++.              .+++++.    ",
  "                         .++++++++++++.               .+++++.   ",
  "                        .+++++++++++++.                .+++++.  ",
  "                       .+++++++++++++.                  .+++++. ",
  "                      .+++++++++++++.                    .++++. ",
  "                     .+++++++++++++.                      ....  ",
  "                    .+++++++++++++.                             ",
  "                   .+++++++++++++.                              ",
  "                  .+++++++++++++.                               ",
  "                 .+++++++++++++.                                ",
  "                .+++++++++++++.                                 ",
  "               .+++++++++++++.                                  ",
  "              .+++++++++++++.                                   ",
  "             .+++++++++++++.                                    ",
  "            .+++++++++++++.                                     ",
  "           .+++++++++++++.                                      ",
  "          .+++++++++++++.                                       ",
  "         .+++++++++++++.                                        ",
  "        .+++++++++++++.                                         ",
  "       .+++++++++++++.                                          ",
  "       .++++++++++++.                                           ",
  "       .+++++++++++.                                            ",
  "       .++++++++++.                                             ",
  "       .+++++++++.                                              ",
  "      .+++++++++.                                               ",
  "     .+++++.....                                                ",
  "    .+++++.                                                     ",
  "   .+++++.                                                      ",
  "  .+++++.                                                       ",
  " .+++++.                                                        ",
  " .++++.                                                         ",
  " .+++.                                                          ",
  " .+..                                                           ",
  "                                                                ",
  "                                                                ",
];

/** `resources/bitmaps_png/cursors/current_probe_black64.xpm` ('.' white, '+' black). */
const CURRENT_PROBE_64 = [
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                             ......             ",
  "                                            .++++++.            ",
  "                                           .++++++++.           ",
  "                                          .+++....+++.          ",
  "                                         .+++.    .+++.         ",
  "                                        .+++.      .+++.        ",
  "                                     ...+++.        .+++.       ",
  "                                    .+++++.          .++.       ",
  "                                 ...+++++.           .++.       ",
  "                                .+++++++.            .++.       ",
  "                               .++++++++.            .++.       ",
  "                              .+++++++++.            .+++.      ",
  "                             .++++++++++.             .+++.     ",
  "                            .+++++++++++.              .+++.    ",
  "              ....         .+++++++++++.                .+++.   ",
  "             .++++.       .+++++++++++.                  .+++.  ",
  "             .++++.      .+++++++++++.                    .+++. ",
  "             .++++.     .+++++++++++.                      ...  ",
  "             .++++.    .+++++++++++.                            ",
  "             .++++.   .+++++++++++.                             ",
  "             .++++.  .+++++++++++.                              ",
  "             .++++. .+++++++++++.                               ",
  "             .++++..+++++++++++.                                ",
  "             .++++.+++++++++++.                                 ",
  "             .+++++++++++++++.                                  ",
  "             .++++++++++++++.                                   ",
  "             .+++++++++++++.                                    ",
  "             .++++++++++++.                                     ",
  "            ..+++++++++++.                                      ",
  "           .++++++++++++.                                       ",
  "       .....+++++++++++.                                        ",
  "      .+++++++++++++++.                                         ",
  "   ...+++++++++++++++.                                          ",
  "  .+++++++++++++++++.                                           ",
  " .+++++++++++++++++.                                            ",
  " .++++......++++++.                                             ",
  " .+++.      .+++++.                                             ",
  ".+++.        .++++.                                             ",
  ".+++.        .++++.                                             ",
  ".+++.        .++++.                                             ",
  ".+++.        .++++.                                             ",
  ".+++.        .++++.                                             ",
  ".+++.        .+++.                                              ",
  " .++.       .+++.                                               ",
  " .+. .......++++.                                               ",
  "  . .+++++++++++.                                               ",
  "   .+++++++++++.                                                ",
  "    ..++++++...                                                 ",
  "      ......                                                    ",
];

/** `resources/bitmaps_png/cursors/cursor_tune_black.xpm` ('.' black, '+' white). */
const TUNE = [
  "                                ",
  "                                ",
  "                                ",
  "                        ..      ",
  "                       ....     ",
  "                      ......    ",
  "                     ........   ",
  "                    .........   ",
  "                   .........    ",
  "                  .........     ",
  "                 .........      ",
  "                .........       ",
  "               .........        ",
  "              .........         ",
  "             .........          ",
  "             ........           ",
  "          ..........            ",
  "          .........             ",
  "          ........              ",
  "           .....                ",
  "          .+....                ",
  "         .+. ...                ",
  "        .+.                     ",
  "       .+.                      ",
  "      .+.                       ",
  "     .+.                        ",
  "    .+.                         ",
  "  ..+.                          ",
  " .++.                           ",
  ".+++.                           ",
  " .+.                            ",
  "  .                             ",
];

/** `resources/bitmaps_png/cursors/cursor_tune_black64.xpm` ('.' black, '+' white). */
const TUNE_64 = [
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                                ",
  "                                                 ..             ",
  "                                                ....            ",
  "                                               ......           ",
  "                                              ........          ",
  "                                             ..........         ",
  "                                            ............        ",
  "                                           ..............       ",
  "                                          ................      ",
  "                                         ..................     ",
  "                                        ...................     ",
  "                                       ...................      ",
  "                                      ...................       ",
  "                                     ...................        ",
  "                                    ...................         ",
  "                                   ...................          ",
  "                                  ...................           ",
  "                                 ...................            ",
  "                                ...................             ",
  "                               ...................              ",
  "                              ...................               ",
  "                             ...................                ",
  "                            ...................                 ",
  "                           ...................                  ",
  "                          ...................                   ",
  "                          ..................                    ",
  "                          .................                     ",
  "                         .................                      ",
  "                    .....................                       ",
  "                    ....................                        ",
  "                    ...................                         ",
  "                    ..................                          ",
  "                    .................                           ",
  "                    ................                            ",
  "                      ...........                               ",
  "                     ...........                                ",
  "                    ..++........                                ",
  "                   ..+++........                                ",
  "                  ..+++.. ......                                ",
  "                 ..+++..  ......                                ",
  "                ..+++..                                         ",
  "               ..+++..                                          ",
  "              ..+++..                                           ",
  "             ..+++..                                            ",
  "            ..+++..                                             ",
  "           ..+++..                                              ",
  "          ..+++..                                               ",
  "         ..+++..                                                ",
  "        ..+++..                                                 ",
  "       ..+++..                                                  ",
  "    ....+++..                                                   ",
  "   ...++++..                                                    ",
  "  ..+++++..                                                     ",
  " ..++++++.                                                      ",
  "..++++++..                                                      ",
  "..++++++..                                                      ",
  " ..++++..                                                       ",
  "  ..++..                                                        ",
  "   ....                                                         ",
  "    ..                                                          ",
];

export type ProbeCursor = "voltage_probe" | "current_probe" | "tune";

/** XPM colours: the probes' '.' is white and '+' black, the tune cursor's the other way round. */
const PROBE_COLOURS: Readonly<Record<string, string>> = { ".": "#FFFFFF", "+": "#000000" };
const TUNE_COLOURS: Readonly<Record<string, string>> = { ".": "#000000", "+": "#FFFFFF" };

/** `cursors_defs`: the picture and hot spot of each, the standard one then the HiDPI one. */
const DEFS: Record<ProbeCursor, { rows: readonly string[]; hotspot: [number, number]; rows64: readonly string[]; hotspot64: [number, number]; colours: Readonly<Record<string, string>> }> = {
  voltage_probe: { rows: VOLTAGE_PROBE, hotspot: [1, 31], rows64: VOLTAGE_PROBE_64, hotspot64: [1, 62], colours: PROBE_COLOURS },
  current_probe: { rows: CURRENT_PROBE, hotspot: [4, 27], rows64: CURRENT_PROBE_64, hotspot64: [8, 54], colours: PROBE_COLOURS },
  tune: { rows: TUNE, hotspot: [1, 30], rows64: TUNE_64, hotspot64: [2, 60], colours: TUNE_COLOURS },
};

/** An XPM's pixels as an SVG: one rectangle per run of a colour along a row (`colours`: the probes' '.' white, '+' black; anything else transparent). */
export function xpmToSvg(rows: readonly string[], colours: Readonly<Record<string, string>> = PROBE_COLOURS): string {
  const width = Math.max(0, ...rows.map((r) => r.length));
  const rects: string[] = [];
  rows.forEach((row, y) => {
    let x = 0;
    while (x < row.length) {
      const c = row[x]!;
      let end = x + 1;
      while (end < row.length && row[end] === c) end++;
      const fill = colours[c];
      if (fill) rects.push(`<rect x="${x}" y="${y}" width="${end - x}" height="1" fill="${fill}"/>`);
      x = end;
    }
  });
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${rows.length}" shape-rendering="crispEdges">${rects.join("")}</svg>`;
}

const url = (rows: readonly string[], colours: Readonly<Record<string, string>>) => `url("data:image/svg+xml,${encodeURIComponent(xpmToSvg(rows, colours))}")`;

/** Whether the browser takes a `cursor` value (`CSS.supports`); true where it cannot be asked. */
function cssSupports(value: string): boolean {
  const css = (globalThis as { CSS?: { supports?: (p: string, v: string) => boolean } }).CSS;
  return css?.supports ? css.supports("cursor", value) : true;
}

/**
 * The CSS `cursor` of a probe or the tune cursor: KiCad's two pictures as one image set at the standard picture's hot spot (`wxCursorBundle`; the 64-pixel one
 * drawn on a screen of twice the density), `-webkit-image-set` where only that is taken, the 32-pixel picture alone where neither is, the crosshair where a
 * browser cannot show an image.
 */
export function probeCursorCss(kind: ProbeCursor, supports: (value: string) => boolean = cssSupports): string {
  const d = DEFS[kind];
  const [x, y] = d.hotspot;
  const one = url(d.rows, d.colours);
  const two = url(d.rows64, d.colours);
  for (const fn of ["image-set", "-webkit-image-set"]) {
    const value = `${fn}(${one} 1x, ${two} 2x) ${x} ${y}, crosshair`;
    if (supports(value)) return value;
  }
  return `${one} ${x} ${y}, crosshair`;
}
