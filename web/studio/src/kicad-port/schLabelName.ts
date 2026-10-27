// A local or global label put down on a wire that a label already names takes that name, with no dialog: `SCH_DRAWING_TOOLS::createNewLabel`
// (eeschema/tools/sch_drawing_tools.cpp, 25ec2335):
//
//   case LAYER_LOCLABEL: ... if( SCH_LINE* wire = findWire( aPosition ) ) netName = findWireLabelDriverName( wire );
//   case LAYER_GLOBLABEL: ... (the same)
//   if( !netName.IsEmpty() ) labelItem->SetText( netName );          // "Auto-create from attached wire"
//   else { DIALOG_LABEL_PROPERTIES dlg( .. ); .. }
//
// `findWire` is the first wire (not a bus, not a graphic line) whose box holds the point; `findWireLabelDriverName` is the wire's connection's
// `LocalName()` when its driver is a local or a global label (`driver->IsType( { SCH_LABEL_T, SCH_GLOBAL_LABEL_T } )`), else nothing. The driver is
// the server's to know (it traces the nets): `SchematicWire.label_driver`. A hierarchical label and a directive label always open the dialog.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd, LabelScope, LabelShape, SchematicWire } from "../api/types";
import type { LabelSpinName } from "./schLastPlaced";

/** `SCH_LINE::GetBoundingBox`'s inflation for a wire of the default width: half the 6 mil pen (`GetPenWidth() / 2`), in nanometres. */
const WIRE_HALF_PEN_NM = 76_200;

/** `findWire( aPosition )`: the first wire of the sheet whose box holds the point (a bus is not a wire: `line->IsWire()`). */
export function findWire<W extends Pick<SchematicWire, "pts" | "bus">>(wires: readonly W[], at: readonly [number, number]): W | null {
  const [x, y] = at;
  for (const w of wires) {
    if (w.bus) continue;
    for (let i = 0; i + 1 < w.pts.length; i++) {
      const [x1, y1] = w.pts[i]!;
      const [x2, y2] = w.pts[i + 1]!;
      if (x >= Math.min(x1, x2) - WIRE_HALF_PEN_NM && x <= Math.max(x1, x2) + WIRE_HALF_PEN_NM && y >= Math.min(y1, y2) - WIRE_HALF_PEN_NM && y <= Math.max(y1, y2) + WIRE_HALF_PEN_NM) return w;
    }
  }
  return null;
}

/** The name a new label of `scope` put down at `at` takes from the wire there, or null when the dialog is to ask (`createNewLabel`). */
export function labelNameFromWire(wires: readonly Pick<SchematicWire, "pts" | "bus" | "label_driver">[], at: readonly [number, number], scope: LabelScope): string | null {
  if (scope !== "local" && scope !== "global") return null;
  const name = findWire(wires, at)?.label_driver;
  return name ? name : null;
}

/** What a label dialog (or a label named from its wire) sends: one `place_label` command. */
export interface LabelPlacement {
  at: readonly [number, number];
  scope: LabelScope;
  text: string;
  shape: LabelShape;
  /** The spin chosen, "" for "From the wire". */
  spin: LabelSpinName | "";
  /** The spin read off the wire when `spin` is "" (null for none). */
  plainSpin: LabelSpinName | null;
  sizeNm: number | null;
  bold: boolean;
  italic: boolean;
  /** The "Auto" check is on, for a global or hierarchical label. */
  autoRotate: boolean;
}

export function placeLabelCmd(p: LabelPlacement): Cmd {
  const kind = p.scope === "local" ? ({ scope: "local" } as const) : ({ scope: p.scope, shape: p.shape } as const);
  return {
    op: "sch_edit",
    verb: "place_label",
    net: p.text,
    at: { x: p.at[0], y: p.at[1] },
    kind,
    ...(p.spin || p.plainSpin ? { spin: (p.spin || p.plainSpin) as LabelSpinName } : {}),
    ...(p.sizeNm !== null && p.sizeNm !== 1_270_000 ? { size_nm: p.sizeNm } : {}),
    ...(p.bold ? { bold: true } : {}),
    ...(p.italic ? { italic: true } : {}),
    ...(p.autoRotate ? { auto_rotate: true } : {}),
  } as Cmd;
}
