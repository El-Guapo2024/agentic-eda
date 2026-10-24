// A group on the sheet (`SCH_PAINTER::draw( const SCH_GROUP* )`, eeschema/sch_painter.cpp at 8303b2ad): nothing at the group's level unless it is selected on its own (a selected
// group whose parent is not selected too) or is the group being worked in; then a box around everything below it, and above the box a tab with the name when the name fits
// the box's width. The members are drawn as they always are.
import type { Schematic } from "../../api/types";
import type { ViewTransform } from "../../state/store";
import { layerColor } from "../canvas/layers";
import { drawStrokeText } from "../text/strokeFont";
import { groupsOf } from "../../kicad-port/schGroups";
import { groupBounds, sheetBoxOf } from "./schGroupBounds";
import { parentGroup } from "../../kicad-port/groupTree";

/** 12 mil, `schIUScale.MilsToIU( ptSize )` for the name's size, micrometres. */
const NAME_UNSCALED_NM = 304_800;
/** `ptSize = 12`: the name is about this many pixels high before it is blended with the unscaled size. */
const NAME_PT = 12;

export function drawGroups(ctx: CanvasRenderingContext2D, view: ViewTransform, sch: Schematic, selection: ReadonlySet<string>, entered: string | null): void {
  const groups = groupsOf(sch);
  if (groups.length === 0) return;
  const shown = groups.filter((g) => {
    const above = parentGroup(groups, g.id);
    return (selection.has(g.id) && !(above && selection.has(above.id))) || entered === g.id;
  });
  if (shown.length === 0) return;
  const boxOf = sheetBoxOf(sch);
  const scale = view.scale || 1;
  const hair = 1 / scale;
  ctx.save();
  ctx.strokeStyle = layerColor("LAYER_SCHEMATIC_ANCHOR");
  ctx.lineWidth = Math.max(hair * 2, 100_000);
  ctx.lineJoin = "miter";
  ctx.setLineDash([]);
  for (const g of shown) {
    const box = groupBounds(sch, g.id, boxOf);
    if (!box) continue;
    const w = box.maxX - box.minX;
    ctx.strokeRect(box.minX, box.minY, w, box.maxY - box.minY);
    if (!g.name) continue;
    // "Scale by zoom a bit, but not too much": a third of the on-screen size and two thirds of the unscaled one
    const textSize = (NAME_PT * hair + NAME_UNSCALED_NM * 2) / 3;
    if ([...g.name].length * textSize >= w) continue;
    const title = textSize * 2;
    ctx.beginPath();
    ctx.moveTo(box.minX, box.minY);
    ctx.lineTo(box.minX, box.minY - title);
    ctx.lineTo(box.maxX, box.minY - title);
    ctx.lineTo(box.maxX, box.minY);
    ctx.stroke();
    // italic, centred on the box, its foot half a text height above the box's top edge (`textOffset = ( width / 2, -textSize * 0.5 )`, bottom aligned)
    drawStrokeText(ctx, g.name, box.minX + w / 2, box.minY - textSize * 0.5, { sizeNm: textSize, justify: "center", italic: true, color: layerColor("LAYER_SCHEMATIC_ANCHOR") });
  }
  ctx.restore();
}
