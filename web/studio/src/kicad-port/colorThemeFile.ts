// KiCad's colour theme files (`colors/<name>.json` in the user's configuration folder) read and written as KiCad 8303b2ad does: `COLOR_SETTINGS`
// (common/settings/color_settings.cpp) over `JSON_SETTINGS` (common/settings/json_settings.cpp), a colour being a `COLOR4D` written by
// `to_json( nlohmann::json&, const COLOR4D& )` -> `COLOR4D::ToCSSString` and read by `from_json` -> `COLOR4D::SetFromWxString` -> `wxColour::Set`
// (wxWidgets 3.2 src/common/colourcmn.cpp, `wxColourBase::FromString`).
//
//   * A file is one JSON object. A param's path is its key with `.` for nesting (`JSON_SETTINGS_INTERNALS::PointerFromString`: "board.copper.f" is
//     `{ "board": { "copper": { "f": .. } } }`). `meta.name` is the theme's name (default "KiCad Default"), `meta.version` the schema version (6 in
//     8303b2ad, `colorsSchemaVersion`), `meta.filename` the file's name.
//   * Reading (`JSON_SETTINGS::LoadFromFile`, then `Load`): a file older than version 6 is migrated first (`registerMigration`: 0 -> 1 splits the
//     footprint editor's `fpedit` namespace into a theme of its own, 1 -> 2 sets the via holes' colour, 2 -> 3 makes six 3D colours opaque, 3 -> 4 gives
//     the page limits the grid's colour, 5 -> 6 makes DRC highlighted magenta); then each param takes the file's value, a param the file lacks keeps
//     KiCad's default (`COLOR_MAP_PARAM::Load` with `aResetIfMissing`). A colour that is not a string is the default too (`Get<COLOR4D>` gives nothing);
//     a string `wxColour` cannot read is black (`COLOR4D()`, the value `from_json` leaves).
//   * Writing (`SaveToFile`): the whole file, every key sorted (`nlohmann::json` is a `std::map`), indented by two (`std::setw( 2 )`), with what was
//     read and not understood kept (`toSave.update( m_internals )`: the 3D viewer's and the Gerber viewer's colours, keys of older versions).
//
// Not read: a colour by its wxWidgets name (`wxTheColourDatabase`: "red", "light grey", ...) -- black, as a string `FromString` rejects is; comments in the
// file (nlohmann's `ignore_comments`) -- the file is refused. Pure: unit-tested in colorThemeFile.test.ts against a theme KiCad wrote (fixtures/nord.json).
import type { ColorTheme, StoredTheme, ThemeData } from "./colorTheme";

/** `colorsSchemaVersion`. */
export const COLORS_SCHEMA_VERSION = 6;

/** One `COLOR_MAP_PARAM`: its JSON path and the layer it is the colour of. */
export interface ThemeParam {
  key: string;
  layer: string;
}

const hex2 = (n: number) => n.toString(16).padStart(2, "0");
const clip = (v: number) => Math.min(255, Math.max(0, v));
/** `wxRound`: half away from zero. */
const wxRound = (v: number) => (v < 0 ? -Math.round(-v) : Math.round(v));

/**
 * `wxColourBase::FromString`: "rgb( r , g , b )", "rgba( r , g , b , a )" (any case of "rgb"; the alpha a C-locale double, `wxRound( a * 255 )`), each
 * channel clipped to 0 .. 255; "#rrggbb", "#rrggbbaa", "#rgb", "#rgba". As `#rrggbbaa`, or null when it cannot be read (or is a colour name).
 */
export function parseColorString(text: string): string | null {
  if (text === "") return null;
  if (text.slice(0, 3).toLowerCase() === "rgb") {
    let r: number, g: number, b: number;
    let a = 255;
    if (text[3] === "a" || text[3] === "A") {
      // sscanf( str + 4, "( %d , %d , %d , %<len>[^)] )" ) must fill all four
      const m = /^\(\s*([+-]?\d+)\s*,\s*([+-]?\d+)\s*,\s*([+-]?\d+)\s*,\s*([^)]+)/.exec(text.slice(4));
      if (!m) return null;
      const alphaStr = m[4]!.replace(/\s+$/, "");
      // `ToCDouble`: the whole text a number
      if (!/^\s*[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(alphaStr)) return null;
      [r, g, b] = [Number(m[1]), Number(m[2]), Number(m[3])];
      a = wxRound(Number(alphaStr) * 255);
    } else {
      const m = /^\(\s*([+-]?\d+)\s*,\s*([+-]?\d+)\s*,\s*([+-]?\d+)/.exec(text.slice(3));
      if (!m) return null;
      [r, g, b] = [Number(m[1]), Number(m[2]), Number(m[3])];
    }
    return `#${hex2(clip(r))}${hex2(clip(g))}${hex2(clip(b))}${hex2(clip(a))}`;
  }
  if (text[0] === "#") {
    // wxSscanf( str + 1, "%lx" ) reads the hex digits it can; the length is the whole text's
    const digits = /^[0-9a-fA-F]+/.exec(text.slice(1))?.[0];
    if (!digits) return null;
    let tmp = BigInt(`0x${digits}`);
    const len = text.length - 1;
    const byte = (shift: number) => hex2(Number((tmp >> BigInt(shift)) & 0xffn));
    const nib = (shift: number) => hex2(Number((tmp >> BigInt(shift)) & 0xfn) * 0x11);
    // case 6: `tmp = ( tmp << 8 ) + wxALPHA_OPAQUE`, then case 8; case 3: `tmp = ( tmp << 4 ) + 0xF`, then case 4
    if (len === 6) tmp = (tmp << 8n) + 0xffn;
    if (len === 6 || len === 8) return `#${byte(24)}${byte(16)}${byte(8)}${byte(0)}`;
    if (len === 3) tmp = (tmp << 4n) + 0xfn;
    if (len === 3 || len === 4) return `#${nib(12)}${nib(8)}${nib(4)}${nib(0)}`;
    return null;
  }
  return null;
}

/** `COLOR4D::ToCSSString`: "rgb(r, g, b)" when opaque, else "rgba(r, g, b, a)" with the alpha to three places (`wxString::FromCDouble( alpha / 255.0, 3 )`). */
export function toCssString(hex8: string): string {
  const v = (i: number) => parseInt(hex8.slice(i, i + 2), 16);
  const [r, g, b] = [v(1), v(3), v(5)];
  const a = hex8.length >= 9 ? v(7) : 255;
  return a === 255 ? `rgb(${r}, ${g}, ${b})` : `rgba(${r}, ${g}, ${b}, ${(a / 255).toFixed(3)})`;
}

type Json = Record<string, unknown>;
const isObject = (v: unknown): v is Json => !!v && typeof v === "object" && !Array.isArray(v);

function getPath(root: Json, path: string): unknown {
  let at: unknown = root;
  for (const part of path.split(".")) {
    if (!isObject(at) || !(part in at)) return undefined;
    at = at[part];
  }
  return at;
}

/** `( *m_internals )[ PointerFromString( path ) ] = value`: the objects on the way are made as needed. */
function setPath(root: Json, path: string, value: unknown): void {
  const parts = path.split(".");
  let at: Json = root;
  for (const part of parts.slice(0, -1)) {
    if (!isObject(at[part])) at[part] = {};
    at = at[part] as Json;
  }
  at[parts[parts.length - 1]!] = value;
}

function deletePath(root: Json, path: string): void {
  const parts = path.split(".");
  const chain: Json[] = [root];
  for (const part of parts.slice(0, -1)) {
    const next = chain[chain.length - 1]![part];
    if (!isObject(next)) return;
    chain.push(next);
  }
  delete chain[chain.length - 1]![parts[parts.length - 1]!];
  // an object left empty by it goes too, so a theme with nothing but its colours keeps nothing
  for (let i = chain.length - 1; i > 0; i--) if (Object.keys(chain[i]!).length === 0) delete chain[i - 1]![parts[i - 1]!];
}

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

/** `std::optional<COLOR4D> Get<COLOR4D>( path )`: nothing when the path is absent or not a string; black for a string `FromString` rejects. */
function getColor(root: Json, path: string): string | undefined {
  const v = getPath(root, path);
  if (typeof v !== "string") return undefined;
  return parseColorString(v) ?? "#000000ff";
}

/** The migrations of `COLOR_SETTINGS::COLOR_SETTINGS` from `version` up to `COLORS_SCHEMA_VERSION`, on the file's JSON (1 -> 6; 0 -> 1 is `splitFootprintTheme`). */
function migrate(root: Json, version: number): void {
  if (version < 2) {
    // "Fix LAYER_VIA_HOLES color - before version 2, this setting had no effect": COLOR4D( 0.5, 0.4, 0, 0.8 ).ToCSSString()
    setPath(root, "board.via_hole", "rgba(128, 102, 0, 0.800)");
  }
  if (version < 3) {
    // "We don't support opacity in some 3D colors but some versions of 5.99 let you set it."
    for (const path of ["3d_viewer.background_top", "3d_viewer.background_bottom", "3d_viewer.copper", "3d_viewer.silkscreen_top", "3d_viewer.silkscreen_bottom", "3d_viewer.solderpaste"]) {
      const c = getColor(root, path);
      if (c) setPath(root, path, toCssString(`${c.slice(0, 7)}ff`));
    }
  }
  if (version < 4) {
    const board = getColor(root, "board.grid");
    if (board) setPath(root, "board.page_limits", toCssString(board));
    const sch = getColor(root, "schematic.grid");
    if (sch) setPath(root, "schematic.page_limits", toCssString(sch));
  }
  // 4 -> 5: "this bump shouldn't have happened; add a no-op migration to avoid future issues"
  if (version < 6) setPath(root, "board.drc_highlighted", toCssString("#ff00ffff")); // COLOR4D( PUREMAGENTA )
  setPath(root, "meta.version", COLORS_SCHEMA_VERSION);
}

/** The theme a (migrated) file's JSON holds: its params' colours, its name and override flag, and what else the file has (`extra`). */
function themeOf(root: Json, filename: string, params: readonly ThemeParam[]): StoredTheme {
  const colors: Record<string, string> = {};
  const extra = clone(root);
  for (const p of params) {
    const c = getColor(root, p.key);
    if (c !== undefined) colors[p.layer] = c;
    deletePath(extra, p.key);
  }
  const name = getPath(root, "meta.name");
  const override = getPath(root, "schematic.override_item_colors");
  for (const path of ["meta.name", "meta.version", "meta.filename", "schematic.override_item_colors"]) deletePath(extra, path);
  const theme: StoredTheme = { filename, name: typeof name === "string" ? name : "KiCad Default", colors };
  if (typeof override === "boolean") theme.overrideSchItemColors = override;
  if (Object.keys(extra).length > 0) theme.extra = extra;
  return theme;
}

/** The text of a theme file read: the themes it gives (one, or two for a version-0 file with an `fpedit` namespace), or why it is not a theme. */
export interface ParsedThemeFile {
  themes: StoredTheme[];
  error?: string;
}

/**
 * A theme file read as `SETTINGS_MANAGER` reads `colors/<filename>.json` (`loadColorSettingsByName`: the theme's filename is the file's, not `meta.filename`).
 * `params` are the board's and the schematic's `COLOR_MAP_PARAM`s (src/kicad/color_themes.json).
 */
export function parseThemeFile(text: string, filename: string, params: readonly ThemeParam[]): ParsedThemeFile {
  let root: unknown;
  try {
    root = JSON.parse(text);
  } catch {
    return { themes: [], error: "The file is not a KiCad color theme (it is not JSON)." };
  }
  if (!isObject(root)) return { themes: [], error: "The file is not a KiCad color theme." };
  const raw = getPath(root, "meta.version");
  // `Get<int>( "meta.version" )`; a file without one is not migrated (`filever` stays -1)
  const version = typeof raw === "number" && Number.isInteger(raw) ? raw : -1;
  const out: StoredTheme[] = [];
  if (version >= 0 && version < 1 && isObject(root.fpedit)) {
    // `migrateSchema0to1`: "Footprint editor settings are split out into a new file called "ThemeName (Footprints)"": a clone with `board` = `fpedit`,
    // named `GetName() + " (Footprints)"`, filed as `GetFilename().BeforeLast( '.' ) + "_footprints"` -- the filename has no extension, so that is
    // "_footprints" in 8303b2ad.
    const fp = clone(root);
    fp.board = fp.fpedit;
    delete fp.fpedit;
    delete root.fpedit;
    migrate(fp, 1);
    const fpTheme = themeOf(fp, `${filename.includes(".") ? filename.slice(0, filename.lastIndexOf(".")) : ""}_footprints`, params);
    fpTheme.name = `${fpTheme.name} (Footprints)`;
    out.push(fpTheme);
  }
  if (version >= 0 && version < COLORS_SCHEMA_VERSION) migrate(root, Math.max(version, 1));
  out.unshift(themeOf(root, filename, params));
  return { themes: out };
}

/** Keys sorted at every level, as `nlohmann::json` keeps an object. */
function sortKeys(v: unknown): unknown {
  if (Array.isArray(v)) return v.map(sortKeys);
  if (!isObject(v)) return v;
  const out: Json = {};
  for (const k of Object.keys(v).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0))) out[k] = sortKeys(v[k]);
  return out;
}

/**
 * `COLOR_SETTINGS::SaveToFile`: the theme as KiCad writes `colors/<filename>.json` -- `meta` (filename, name, version 6), `schematic.override_item_colors`,
 * every board and schematic param (the theme's colour, else KiCad's default), and the entries the file it was read from had besides (`extra`).
 */
export function writeThemeFile(theme: ColorTheme, params: readonly ThemeParam[], data: ThemeData): string {
  const out: Json = isObject(theme.extra) ? clone(theme.extra) : {};
  setPath(out, "meta.filename", `${theme.filename}.json`);
  setPath(out, "meta.name", theme.name);
  setPath(out, "meta.version", COLORS_SCHEMA_VERSION);
  setPath(out, "schematic.override_item_colors", theme.overrideSchItemColors ?? false);
  for (const p of params) {
    const c = theme.colors[p.layer] ?? data.defaults[p.layer];
    if (c) setPath(out, p.key, toCssString(c));
  }
  return `${JSON.stringify(sortKeys(out), null, 2)}\n`;
}

/** A picked file's name as a theme's filename (`wxFileName::GetName()`: without the directory and the extension). */
export function themeFilenameOf(fileName: string): string {
  const base = fileName.replace(/^.*[\\/]/, "");
  return base.toLowerCase().endsWith(".json") ? base.slice(0, -5) : base;
}
