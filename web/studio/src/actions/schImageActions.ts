// The schematic's image tools: Place Image (`SCH_DRAWING_TOOLS::PlaceImage`) and the drop of an image file on the sheet (`SCH_EDIT_TOOL::DdAddImage`, which hands the loaded image to the
// same tool). The file is chosen in the browser's chooser (or comes with the drop), the image follows the cursor by its centre -- drawn by SchematicView from state/schImageHand.ts -- and a
// click puts it on the sheet at the grid (`GRID_GRAPHICS`) as one undo step (`commit.Push( _( "Place Image" ) )`), selected. As on the board (actions/pcbItemActions.ts) the tool is a run of picker
// sessions: the next click on the sheet answers it, Escape ends it.
import { commitGraphic } from "../components/schematic/schShapeTools";
import { SCALE_PPM } from "../kicad-port/refImage";
import { setSchImageHand } from "../state/schImageHand";
import { asPickedImage, chooseImageFile, loadPickedImage, readImageFile, type LoadedImage, type PickedImage } from "./imageFiles";
import { picker } from "./pcbPicker";
import { schematicActions, type ActionMap } from "./schActionRegistry";
import type { SchEditContext } from "./schEditActions";

export function registerSchImageActions(registry: ActionMap, ctx: SchEditContext): void {
  const m = schematicActions(registry, ctx.state.tab);
  // A parameter that is a picked image ({ name, data }) is the file already chosen: no chooser (a test).
  m.set("eeschema.InteractiveDrawing.placeImage", (param) => startPlaceImage(ctx, asPickedImage(param), false));
  // `DdAddImage` runs `placeImage` with the loaded image: immediate mode -- one click places it and the tool ends. A drop with no file in it places nothing.
  m.set("eeschema.EditorControl.ddAddImage", (param) => {
    const file = asPickedImage(param);
    if (file) startPlaceImage(ctx, file, true);
  });
}

/**
 * `PlaceImage`. Without a file the tool asks for one at once (the "immediate actions" KiCad starts with); a file that is no image says so (`_( "Could not load image from '%s'." )`). With the
 * image in the hand a click drops it; the tool then waits for a click that asks for another file, unless `immediate` (a dropped file), which ends it. Escape drops the image and, a second time,
 * ends the tool.
 */
function startPlaceImage(ctx: SchEditContext, given: PickedImage | null, immediate: boolean): void {
  const { api, dispatch } = ctx;
  const toast = (message: string, kind: "info" | "error" = "info") => dispatch({ type: "TOAST", message, kind });
  const end = () => setSchImageHand(null);

  const armed = (): void => {
    if (immediate) return;
    picker.start({
      kind: "point",
      prompt: "Place Image: click to choose an image file (Escape ends the tool)",
      snap: "graphics",
      onPoint: () => {
        void choose();
        return false;
      },
    });
  };

  const hold = (loaded: LoadedImage): void => {
    setSchImageHand({ name: loaded.name, data: loaded.data, img: loaded.img, px: loaded.header.px, ppi: loaded.header.ppi });
    picker.start({
      kind: "point",
      prompt: `Place Image: click to put ${loaded.name} down (Escape drops it)`,
      snap: "graphics",
      onPoint: (at) => {
        end();
        const sch = api.getState().schematic;
        if (sch) commitGraphic({ sch, dispatch, api }, { shape: { type: "image", at: { x: at.x, y: at.y }, scale_ppm: SCALE_PPM, data: loaded.data } });
        armed();
        return false;
      },
      onCancel: (activated) => {
        end();
        if (!activated && !immediate) armed();
      },
    });
  };

  const load = async (picked: PickedImage): Promise<void> => {
    try {
      hold(await loadPickedImage(picked));
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), "error");
      armed();
    }
  };

  const choose = async (): Promise<void> => {
    const file = await chooseImageFile();
    if (!file) return armed();
    try {
      await load(await readImageFile(file));
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), "error");
      armed();
    }
  };

  dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
  if (given) void load(given);
  else void choose();
}
