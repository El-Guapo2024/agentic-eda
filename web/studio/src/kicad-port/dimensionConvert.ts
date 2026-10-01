// Task item 7: converts `state()`'s own flat display `Dimension` (api/
// types.ts -- [x,y] pairs, a flat `kind` string plus only the fields that
// kind has) back into `CmdDimension`, the shape `add_dimension`/
// `edit_dimension` actually need (`PointXY` objects, a nested tagged
// `kind`) -- the same split `Shape`/`CmdShape` already have, see either
// type's own doc comment in api/types.ts. Pure, dependency-free, so it's
// usable from both useActionRunner.ts (the "switch dimension arrows"
// action) and DimensionPropertiesDialog.tsx without either importing the
// other.
import type { CmdDimension, CmdDimensionKind, Dimension, DimensionSettings } from "../api/types";

export function cmdDimensionKindOf(dim: Pick<Dimension, "kind" | "height" | "horizontal" | "leader_length">): CmdDimensionKind {
  switch (dim.kind) {
    case "aligned":
      return { kind: "aligned", height: dim.height ?? 0 };
    case "orthogonal":
      return { kind: "orthogonal", height: dim.height ?? 0, horizontal: dim.horizontal ?? true };
    case "radial":
      return { kind: "radial", leader_length: dim.leader_length ?? 0 };
    case "leader":
      return { kind: "leader" };
    case "center":
      return { kind: "center" };
  }
}

/** `pcbnew.InteractiveDrawing.*Dimension`/`leader`'s own two-click (start,
 * end) placement, filled in from Board Setup's `DimensionSettings` (task
 * item 7) -- a non-degenerate starting `height`/`leader_length` the
 * properties dialog that opens right after creation lets the user tune,
 * standing in for source's own third interactive "set height" click (see
 * PARITY-pcb.md section 18 on why that's not ported). Orthogonal's
 * `horizontal` defaults to whichever axis the two clicks actually spanned
 * more of -- a reasonable guess from the gesture itself, not a fixed default. */
export function defaultDimensionPayload(kind: CmdDimensionKind["kind"], start: [number, number], end: [number, number], layer: string, settings: DimensionSettings): CmdDimension {
  const DEFAULT_HEIGHT_UM = 5000;
  const DEFAULT_LEADER_LENGTH_UM = 3000;
  const kindObj: CmdDimensionKind =
    kind === "aligned"
      ? { kind: "aligned", height: DEFAULT_HEIGHT_UM }
      : kind === "orthogonal"
        ? { kind: "orthogonal", height: DEFAULT_HEIGHT_UM, horizontal: Math.abs(end[0] - start[0]) >= Math.abs(end[1] - start[1]) }
        : kind === "radial"
          ? { kind: "radial", leader_length: DEFAULT_LEADER_LENGTH_UM }
          : kind === "leader"
            ? { kind: "leader" }
            : { kind: "center" };

  return {
    layer,
    kind: kindObj,
    start: { x: start[0], y: start[1] },
    end: { x: end[0], y: end[1] },
    prefix: "",
    suffix: "",
    override_text: null,
    units: settings.units,
    units_format: settings.units_format,
    precision: settings.precision,
    suppress_trailing_zeros: settings.suppress_trailing_zeros,
    text_position: settings.text_position,
    keep_text_aligned: settings.keep_text_aligned,
    text_angle: 0,
    text_size_um: settings.text_size_um,
    stroke_width: settings.stroke_width,
    arrow_length: settings.arrow_length,
    extension_offset: settings.extension_offset,
    extension_height: settings.extension_height,
    arrow_direction: "outward",
  };
}

export function toCmdDimension(dim: Dimension): CmdDimension {
  return {
    id: dim.id,
    layer: dim.layer,
    kind: cmdDimensionKindOf(dim),
    start: { x: dim.start[0], y: dim.start[1] },
    end: { x: dim.end[0], y: dim.end[1] },
    prefix: dim.prefix,
    suffix: dim.suffix,
    override_text: dim.override_text,
    units: dim.units,
    units_format: dim.units_format,
    precision: dim.precision,
    suppress_trailing_zeros: dim.suppress_trailing_zeros,
    text_position: dim.text_position,
    keep_text_aligned: dim.keep_text_aligned,
    text_angle: dim.text_angle,
    text_size_um: dim.text_size_um,
    stroke_width: dim.stroke_width,
    arrow_length: dim.arrow_length,
    extension_offset: dim.extension_offset,
    extension_height: dim.extension_height,
    arrow_direction: dim.arrow_direction,
  };
}
