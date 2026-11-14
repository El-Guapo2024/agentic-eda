// The Pin Table's rules (`eeschema/dialogs/dialog_lib_edit_pin_table.cpp`, `SYMBOL_EDITOR_EDIT_TOOL::PinTable`), pure so the unit tests can pin them:
//
// - The dialog edits copies of the pins (`TransferDataToWindow`: `m_pins.push_back( new SCH_PIN( *pin ) )`); OK replaces the symbol's pins with them
//   (`TransferDataFromWindow`) and the tool pushes one commit, "Edit Pins". Here OK is one batch of the verbs that turn the old pins into the edited
//   ones (`pinTableCommands`), so one Ctrl+Z undoes the whole table. The table acts on its own rows only: no Synchronized Pins Mode
//   (`PIN_INFO_FORMATTER::UpdatePin` never calls `SynchronizePins`).
// - The Unit and Body Style cells are `GRID_CELL_COMBOBOX`es of "ALL" and the symbol's units ("Unit A", or the unit's display name) and of "ALL",
//   "Standard", "Alternate" (`unitChoices`, `bodyStyleChoices`); on a symbol of one unit or one body style the cell says "ALL" and takes nothing
//   (`UpdatePin`: `if( !parent->IsMultiUnit() ) break;`).
// - A length edit is `SCH_PIN::ChangeLength` (the body end stays, the connection point moves); X and Y are `SetPosition` as typed.
// - Y is KiCad's internal Y, which points down: the file's Y negated (`parseSymbolPin`'s `parseXY( true )`; eeschema's `ORIGIN_TRANSFORMS` is the
//   identity), so the cell shows `-at.y` and a typed value is stored negated (`displayY`, `storedY`).
// - Add Pin is `OnAddRow` (`addedPin`).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd, LibrarySymbol, LibrarySymbolPin } from "../api/types";
import { REPEAT_PIN_STEP_MM } from "./symEditItems";
import { unitLetter } from "./unitLetter";
import { angleDegToOrientation } from "./pinOrientation";
import { bodyStyleCount, bodyStyleDescription, isMultiBodyStyle } from "./symBodyStyles";

/** `UNITS_ALL` / `DEMORGAN_ALL` (symbol_edit_frame.h): `_HKI( "ALL" )`. */
export const ALL = "ALL";

export interface Choice {
  value: number;
  label: string;
}

/** `LIB_SYMBOL::GetUnitDisplayName( unit, true )`: the unit's display name, else "Unit A", "Unit B", ... */
export function unitDisplayName(sym: Pick<LibrarySymbol, "kicad">, unit: number): string {
  const names = (sym.kicad?.unit_names ?? undefined) as Record<string, string> | undefined;
  return names?.[String(unit)] ?? `Unit ${unitLetter(unit)}`;
}

/** The Unit cell's choices: "ALL" (0), then each unit (`for( int i = 1; i <= aSymbol->GetUnitCount(); i++ )`). */
export function unitChoices(sym: Pick<LibrarySymbol, "unit_count" | "kicad">): Choice[] {
  const out: Choice[] = [{ value: 0, label: ALL }];
  for (let u = 1; u <= Math.max(1, sym.unit_count); u++) out.push({ value: u, label: unitDisplayName(sym, u) });
  return out;
}

/** The Body Style cell's choices: "ALL" (0), then each body style by `GetBodyStyleDescription` -- "Standard" (1) and "Alternate" (2) of a De Morgan symbol, a
 *  custom style's name (`for( int i = 1; i <= aSymbol->GetBodyStyleCount(); i++ )`). */
export function bodyStyleChoices(sym: Pick<LibrarySymbol, "has_alternate_body_style" | "body_style_names">): Choice[] {
  const out: Choice[] = [{ value: 0, label: ALL }];
  if (isMultiBodyStyle(sym)) for (let s = 1; s <= bodyStyleCount(sym); s++) out.push({ value: s, label: bodyStyleDescription(sym, s) });
  return out;
}

/** `IsMultiUnit()` / `IsMultiBodyStyle()`: whether the cell takes a value at all. */
export function unitCellEditable(sym: Pick<LibrarySymbol, "unit_count">): boolean {
  return sym.unit_count > 1;
}
export function bodyStyleCellEditable(sym: Pick<LibrarySymbol, "has_alternate_body_style" | "body_style_names">): boolean {
  return isMultiBodyStyle(sym);
}

/** What the Unit cell shows (`PIN_INFO_FORMATTER::Format`, `COL_UNIT`): "ALL" on a symbol of one unit or for a pin common to all units. */
export function unitCellText(sym: Pick<LibrarySymbol, "unit_count" | "kicad">, pin: Pick<LibrarySymbolPin, "unit">): string {
  return !unitCellEditable(sym) || pin.unit === 0 ? ALL : unitDisplayName(sym, pin.unit);
}

/** The Y a cell shows: KiCad's internal Y, the file's negated. */
export function displayY(fileY: number): number {
  return fileY === 0 ? 0 : -fileY;
}
/** The file Y for a typed cell value. */
export function storedY(shownY: number): number {
  return shownY === 0 ? 0 : -shownY;
}

/**
 * `DIALOG_LIB_EDIT_PIN_TABLE::OnAddRow`: `new SCH_PIN( m_symbol )` -- no number or name, unspecified, a line, pointing right, at the origin, the
 * default length and text sizes (`SYMBOL_EDITOR_SETTINGS::m_Defaults`), unit and body style 0 -- and, when the table has pins, the last row's
 * orientation, type, shape, unit and body style, one `m_Repeat.pin_step` (100 mil) from the last row's place: `pos.y -= step` for a left or right
 * pin, `pos.x += step` for an up or down one (KiCad's y points down, so the file's y grows).
 */
export function addedPin(rows: readonly LibrarySymbolPin[]): LibrarySymbolPin {
  const pin: LibrarySymbolPin = {
    number: "",
    name: "",
    electrical_type: "unspecified",
    shape: "line",
    at: { x: 0, y: 0 },
    angle_deg: 0,
    length_mm: 2.54,
    unit: 0,
    body_style: 0,
    hidden: false,
    name_size_mm: 1.27,
    number_size_mm: 1.27,
  };
  const last = rows[rows.length - 1];
  if (!last) return pin;
  const o = angleDegToOrientation(last.angle_deg);
  const snap = (v: number) => Math.round(v * 1e6) / 1e6;
  const at = o === "left" || o === "right" ? { x: last.at.x, y: snap(last.at.y + REPEAT_PIN_STEP_MM) } : { x: snap(last.at.x + REPEAT_PIN_STEP_MM), y: last.at.y };
  return { ...pin, angle_deg: last.angle_deg, electrical_type: last.electrical_type, shape: last.shape, unit: last.unit, body_style: last.body_style, at };
}

const samePin = (a: LibrarySymbolPin, b: LibrarySymbolPin) => JSON.stringify({ ...a, alternates: a.alternates ?? [] }) === JSON.stringify({ ...b, alternates: b.alternates ?? [] });

/**
 * OK: the verbs that make the symbol's pins the table's rows -- a removed row's pin is deleted, a changed row's pin is edited, a new row (no id) is
 * added -- as one batch, so the whole table is one undo step ("Edit Pins"). Rows the table did not change send nothing.
 */
export function pinTableCommands(libId: string, before: readonly LibrarySymbolPin[], rows: readonly LibrarySymbolPin[]): Cmd[] {
  const kept = new Set(rows.map((r) => r.id).filter((id): id is string => !!id));
  const cmds: Cmd[] = [];
  for (const p of before) if (p.id && !kept.has(p.id)) cmds.push({ op: "delete_symbol_pin", lib_id: libId, id: p.id });
  for (const r of rows) {
    if (!r.id) continue;
    const old = before.find((p) => p.id === r.id);
    if (old && !samePin(old, r)) cmds.push({ op: "edit_symbol_pin", lib_id: libId, id: r.id, pin: r });
  }
  for (const r of rows) if (!r.id) cmds.push({ op: "add_symbol_pin", lib_id: libId, pin: r });
  return cmds;
}

/** A rectangle of the screen (px, the canvas's own corner at 0, 0): a dialog over the canvas. */
export interface ScreenRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * `EDA_DRAW_FRAME::FocusOnLocation( aPos, aAllowScroll = true )` (common/eda_draw_frame.cpp at KiCad 8303b2ad): the view moved to `atNm` when it is off the view or within 10% of
 * its edge (`r.Inflate( -r.GetWidth() / 10.0 )`), or behind a dialog over the canvas or within a tenth of the dialog's width of it (`rect.Inflate( rect.GetWidth() / 10 )` of
 * each of `findDialogs()`; UI-LEFT-3 Y3-5) -- else left as it is (`null`). Moved, the point goes to the middle of the largest part of the canvas the dialogs leave free
 * (`VIEW::SetCenter( aPos, dialogScreenRects )`). `view` maps world nm to screen px (`screen = world * scale + (x, y)`); the canvas is `widthPx` by `heightPx`; `dialogs` are
 * the dialogs' rectangles on the canvas.
 */
export function focusOnLocation(view: { scale: number; x: number; y: number }, atNm: readonly [number, number], widthPx: number, heightPx: number, dialogs: readonly ScreenRect[] = []): { scale: number; x: number; y: number } | null {
  const sx = atNm[0] * view.scale + view.x;
  const sy = atNm[1] * view.scale + view.y;
  // the viewport less a tenth of its width on every side (BOX2D::Inflate by the same amount in x and y)
  const m = widthPx / 10;
  let centre = !(sx >= m && sx <= widthPx - m && sy >= m && sy <= heightPx - m);
  for (const d of dialogs) {
    const g = d.w / 10;
    if (sx >= d.x - g && sx <= d.x + d.w + g && sy >= d.y - g && sy <= d.y + d.h + g) centre = true;
  }
  if (!centre) return null;
  const [cx, cy] = unobscuredCentre(widthPx, heightPx, dialogs);
  return { scale: view.scale, x: cx - atNm[0] * view.scale, y: cy - atNm[1] * view.scale };
}

/**
 * `VIEW::SetCenter( aCenter, obscuringScreenRects )` (common/view/view.cpp): the screen less the dialogs, deflated step by step (`step` a tenth of the free area's box's smaller side,
 * 20 px at least) until nothing is left; the centre of the free area's box before the last step is where the point goes -- the middle of the screen when nothing covers it. The
 * polygon of KiCad is sampled here on a grid of a quarter step, and a deflate with sharp corners (`ALLOW_ACUTE_CORNERS`) of these rectilinear areas is a square's erosion.
 */
export function unobscuredCentre(widthPx: number, heightPx: number, rects: readonly ScreenRect[]): [number, number] {
  if (rects.length === 0 || widthPx <= 0 || heightPx <= 0) return [widthPx / 2, heightPx / 2];
  const covered = (x: number, y: number) => rects.some((r) => x > r.x && x < r.x + r.w && y > r.y && y < r.y + r.h);
  // the free area's box, for the step
  const coarse = Math.max(1, Math.min(widthPx, heightPx) / 200);
  let bx0 = Infinity, by0 = Infinity, bx1 = -Infinity, by1 = -Infinity;
  for (let y = coarse / 2; y < heightPx; y += coarse) {
    for (let x = coarse / 2; x < widthPx; x += coarse) {
      if (covered(x, y)) continue;
      bx0 = Math.min(bx0, x - coarse / 2);
      by0 = Math.min(by0, y - coarse / 2);
      bx1 = Math.max(bx1, x + coarse / 2);
      by1 = Math.max(by1, y + coarse / 2);
    }
  }
  if (bx0 > bx1) return [widthPx / 2, heightPx / 2];
  const step = Math.max(20, Math.min(bx1 - bx0, by1 - by0) / 10);
  const cell = Math.max(1, step / 4);
  const nx = Math.ceil(widthPx / cell);
  const ny = Math.ceil(heightPx / cell);
  // Chebyshev distance (in cells) of each free cell to the nearest covered cell or the screen's edge
  const d = new Float64Array(nx * ny);
  for (let j = 0; j < ny; j++) for (let i = 0; i < nx; i++) d[j * nx + i] = covered((i + 0.5) * cell, (j + 0.5) * cell) ? 0 : Infinity;
  const at = (i: number, j: number) => (i < 0 || j < 0 || i >= nx || j >= ny ? 0 : d[j * nx + i]!);
  for (let j = 0; j < ny; j++) for (let i = 0; i < nx; i++) if (d[j * nx + i]! > 0) d[j * nx + i] = Math.min(d[j * nx + i]!, at(i - 1, j) + 1, at(i, j - 1) + 1, at(i - 1, j - 1) + 1, at(i + 1, j - 1) + 1);
  for (let j = ny - 1; j >= 0; j--) for (let i = nx - 1; i >= 0; i--) if (d[j * nx + i]! > 0) d[j * nx + i] = Math.min(d[j * nx + i]!, at(i + 1, j) + 1, at(i, j + 1) + 1, at(i + 1, j + 1) + 1, at(i - 1, j + 1) + 1);
  let centre: [number, number] = [widthPx / 2, heightPx / 2];
  for (let k = 0; ; k++) {
    // the area deflated k steps: the cells farther than k steps from what bounds it
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (let j = 0; j < ny; j++) {
      for (let i = 0; i < nx; i++) {
        if ((d[j * nx + i]! - 0.5) * cell <= k * step) continue;
        x0 = Math.min(x0, i * cell);
        y0 = Math.min(y0, j * cell);
        x1 = Math.max(x1, Math.min(widthPx, (i + 1) * cell));
        y1 = Math.max(y1, Math.min(heightPx, (j + 1) * cell));
      }
    }
    if (x0 > x1) return centre;
    centre = [(x0 + x1) / 2, (y0 + y1) / 2];
  }
}
