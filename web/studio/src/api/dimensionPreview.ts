// `POST /api/dimension/preview` (crates/cli/src/dimension_api.rs): a dimension the Draw Dimension tool is drawing or the point editor is reshaping, with its
// geometry and text worked out on the server (`eda_connectivity::dimension`, the one place they are), in the shape `GET /api/state` gives a board's dimensions.
// KiCad redraws its preview at every motion (`m_view->Update( &preview )`); the canvas draws the reply in place of the committed dimension (UI-LEFT P13).
import type { CmdDimension, Dimension } from "./types";

/** The dimension `dim` as the board would draw it, or null when the server refuses it. */
export async function fetchDimensionPreview(dim: CmdDimension): Promise<Dimension | null> {
  try {
    const r = await fetch("/api/dimension/preview", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ dimension: dim }) });
    if (!r.ok) return null;
    const v = (await r.json()) as { ok: boolean; dimension?: Dimension };
    return v.ok && v.dimension ? v.dimension : null;
  } catch {
    return null;
  }
}
