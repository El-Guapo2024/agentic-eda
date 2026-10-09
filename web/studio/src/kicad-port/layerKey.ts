// The key the Appearance panel keeps a layer's visibility and opacity under (pure, so the selection code can use it without the colour tables).
/**
 * The key the Appearance panel's visibility and opacity are kept under for a KiCad layer name: copper keeps its own name, the technical layers the panel
 * lists have a painter bucket (`F.SilkS` -> `f_silks`), any other layer (a user layer) is its own name.
 */
export function layerKeyOf(layer: string): string {
  switch (layer) {
    case "F.SilkS":
      return "f_silks";
    case "B.SilkS":
      return "b_silks";
    case "F.Mask":
      return "f_mask";
    case "B.Mask":
      return "b_mask";
    case "F.CrtYd":
      return "f_courtyard";
    case "B.CrtYd":
      return "b_courtyard";
    case "F.Fab":
      return "f_fab";
    case "B.Fab":
      return "b_fab";
    case "Edge.Cuts":
      return "board_edge";
    default:
      return layer;
  }
}
