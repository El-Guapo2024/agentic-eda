// The board editor's tools for the item kinds GAPS.md item 24 (package P16) added: Draw Text Box, Place Reference Image, Place Point and Rehatch Shapes today; Add Barcode as its step
// lands. Called from `registerPcbEditSweep`. Each tool is a run of picker sessions (kicad-port/pickerHost.ts: the next canvas click answers the session, Escape
// ends it) -- `DRAWING_TOOL`'s own loop of clicks is a loop of picks here -- and ends in the item's dialog.
//
//   pcbnew.InteractiveDrawing.textbox               DRAWING_TOOL::DrawRectangle with PCB_ACTIONS::drawTextBox, then DIALOG_TEXTBOX_PROPERTIES (OK adds the box, Cancel throws it away)
//   pcbnew.InteractiveDrawing.placeReferenceImage   DRAWING_TOOL::PlaceReferenceImage: a file chooser, the image follows the cursor by its centre, a click drops it on the active layer
//   pcbnew.InteractiveDrawing.placePoint            DRAWING_TOOL::PlacePoint (`IPO_REPEAT | IPO_SINGLE_CLICK`): each click puts a point of the default size on the active layer, until Escape
//   pcbnew.InteractiveDrawing.barcode               DRAWING_TOOL::DrawBarcode: a click opens Barcode Properties on a QR code of the active layer at the click; OK adds it, Cancel does not; the tool goes on until Escape
//   pcbnew.Control.rehatchShapes                    PCB_CONTROL::RehatchShapes: the hatch lines of every hatched shape worked out again from the board as it is
import { newTextBox } from "../kicad-port/pcbTextBox";
import { activeLayerName } from "../kicad-port/layerPresets";
import { imageSizeUm, SCALE_PPM } from "../kicad-port/refImage";
import { DEFAULT_POINT_SIZE_UM } from "../kicad-port/pcbExtraItems";
import { asPickedImage, chooseImageFile, loadPickedImage, readImageFile, type LoadedImage, type PickedImage } from "./imageFiles";
import { picker } from "./pcbPicker";
import { applyCommands, createSweepHelpers, type SweepCtx } from "./pcbSweepKit";
import { setToolPreview } from "../components/canvas/pcbItemsPaint";
import { openBarcodeDialog, openTextBoxDialog } from "../state/pcbItemDialogs";
import { newBarcode } from "../kicad-port/barcodeShapes";
import { textStyleOf } from "../kicad-port/layerDefaults";
import { gridItemCmd, newGridItem, sizeGridToCursor } from "../kicad-port/gridItem";

export function registerPcbItemActions(m: Map<string, (param?: unknown) => void>, ctx: SweepCtx): void {
  const { pcbOnly } = createSweepHelpers(ctx);

  m.set("pcbnew.InteractiveDrawing.textbox", pcbOnly(() => startTextBoxTool(ctx)));
  // A parameter that is a picked image ({ name, data }) is the file already chosen: no chooser (a test, or a file handed over by something other than the chooser).
  m.set("pcbnew.InteractiveDrawing.placeReferenceImage", (param) => {
    if (ctx.state.tab === "pcb") startPlaceImageTool(ctx, asPickedImage(param));
  });
  m.set("pcbnew.InteractiveDrawing.placePoint", pcbOnly(() => startPlacePointTool(ctx)));
  m.set("pcbnew.InteractiveDrawing.barcode", pcbOnly(() => startBarcodeTool(ctx)));
  // KiCad 10 (25ec2335): `PCB_ACTIONS::placeSubGrid`, "Place Sub-grid".
  m.set("pcbnew.GridItem.place", pcbOnly(() => startPlaceSubGridTool(ctx)));
  // KiCad posts `rehatchShapes` after every commit, undo and file load and keeps the lines in each shape; here the server works them out from the board as it is every time it describes it
  // (`eda_drc::hatch`), so the action is to describe it again -- the lines are never older than the board, and a shape put over a hatched one has already cut its lines.
  m.set("pcbnew.Control.rehatchShapes", pcbOnly(() => void ctx.api.refresh()));
}

/**
 * `DRAWING_TOOL::DrawBarcode`: each click -- where the grid helper holds the cursor, `GRID_TEXT` -- makes a barcode on the active layer with the layer's text size (a 40 mm QR code of level L,
 * `PCB_BARCODE( parent )`) and opens Barcode Properties on it; OK adds it as one undo step ("Draw Barcode") and selects it, Cancel throws it away; after either the tool waits for the next
 * click, until Escape.
 */
function startBarcodeTool(ctx: SweepCtx): void {
  const { api, dispatch } = ctx;
  const layer = () => activeLayerName(api.getState().activeLayer, "F.SilkS");
  const next = (): void => {
    picker.start({
      kind: "point",
      prompt: "Add barcode: click to place it (Escape ends the tool)",
      onPoint: (at) => {
        const board = api.getState().board;
        if (!board) return false;
        const height = textStyleOf(board.board_rules?.text_graphics, layer()).heightUm;
        openBarcodeDialog({ id: null, barcode: newBarcode(layer(), at, height), onClose: next });
        return false;
      },
    });
  };
  dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
  next();
}

/**
 * `DRAWING_TOOL::PlaceSubGrid` (KiCad 10, "Place Sub-grid"): Grids is switched on so the new grid will show; the first click (where the grid helper holds the cursor) is the centre, and
 * the grid then follows the cursor -- as big each way as the cursor is far, turned to face it (`sizeToCursor`) -- until the second click puts it on the board as one undo step ("Place
 * Sub-grid"). Escape drops a grid in the hand and waits for a new centre; Escape again ends the tool.
 */
function startPlaceSubGridTool(ctx: SweepCtx): void {
  const { api, dispatch } = ctx;
  dispatch({ type: "APPEARANCE", op: { op: "object", id: "subgrids", visible: true } });
  dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
  const centre = (): void =>
    picker.start({
      kind: "point",
      prompt: "Place Sub-grid: click the centre of the grid (Escape ends the tool)",
      onPoint: (at) => {
        const item = newGridItem(at.x, at.y);
        setToolPreview({ kind: "grid", item: { ...item, extent: [0, 0] } });
        return {
          kind: "point",
          prompt: "Place Sub-grid: click to set the size and the angle (Escape drops it)",
          onPoint: (to) => {
            setToolPreview(null);
            const placed = sizeGridToCursor(item, [to.x, to.y]);
            void applyCommands(api, dispatch, [{ op: "pcb_item", verb: "add_grid_item", grid_item: gridItemCmd(placed) }]).then(() => centre());
            return false;
          },
          onCancel: (activated) => {
            setToolPreview(null);
            if (!activated) centre();
          },
        };
      },
    });
  centre();
}

/**
 * `DRAWING_TOOL::PlacePoint` through `doInteractiveItemPlacement( .., IPO_REPEAT | IPO_SINGLE_CLICK )`: every click puts a point on the active layer where the grid helper holds the
 * cursor (`BestSnapAnchor`, the picker's point), as one undo step ("Place point"), selected; the tool goes on until Escape.
 */
function startPlacePointTool(ctx: SweepCtx): void {
  const { api, dispatch } = ctx;
  const layer = () => activeLayerName(api.getState().activeLayer, "F.SilkS");
  dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
  picker.start({
    kind: "point",
    prompt: "Place point: click to put a point down (Escape ends the tool)",
    onPoint: (at) => {
      void applyCommands(api, dispatch, [{ op: "pcb_item", verb: "add_point", point: { layer: layer(), at: { x: at.x, y: at.y }, size_um: DEFAULT_POINT_SIZE_UM } }]);
      return true;
    },
  });
}

/**
 * `DRAWING_TOOL::PlaceReferenceImage`. Choosing the tool asks for the file at once (the "immediate actions" setting KiCad starts with: the tool is primed with a click); the image then
 * follows the cursor by its centre, drawn as it will be, and a click puts it on the active layer (`image->SetLayer( m_frame->GetActiveLayer() )`) as one undo step and selects it.
 * The tool goes on: the next click asks for another file; Escape drops an image in the hand, Escape again ends the tool. A file that is no image says so
 * (`_( "Could not load image from '%s'." )`) and the tool waits.
 */
function startPlaceImageTool(ctx: SweepCtx, given: PickedImage | null): void {
  const { api, dispatch } = ctx;
  const toast = (message: string, kind: "info" | "error" = "info") => dispatch({ type: "TOAST", message, kind });
  const layer = () => activeLayerName(api.getState().activeLayer, "Cmts.User");
  const end = () => setToolPreview(null);

  /** The tool waiting for a click that will ask for a file. */
  const armed = (): void =>
    picker.start({
      kind: "point",
      prompt: "Place Reference Image: click to choose an image file (Escape ends the tool)",
      onPoint: () => {
        void choose();
        return false;
      },
    });

  /** An image in the hand: the next click drops it. */
  const hold = (loaded: LoadedImage): void => {
    const [w, h] = imageSizeUm(loaded.header.px, loaded.header.ppi, SCALE_PPM);
    setToolPreview({ kind: "image", img: loaded.img, w, h, layer: layer() });
    picker.start({
      kind: "point",
      prompt: `Place Image: click to put ${loaded.name} down (Escape drops it)`,
      onPoint: (at) => {
        end();
        void applyCommands(api, dispatch, [{ op: "pcb_item", verb: "add_ref_image", image: { layer: layer(), at: { x: at.x, y: at.y }, data: loaded.data } }]).then((ok) => {
          if (!ok) toast("The board refused the image.", "error");
          armed();
        });
        return false;
      },
      onCancel: (activated) => {
        end();
        // Escape throws the image away and the tool waits for the next file (`cleanup()`); another tool taking over ends it.
        if (!activated) armed();
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

  /** The browser's file chooser, then the file. Cancelling it leaves the tool waiting. */
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

/**
 * `DRAWING_TOOL::DrawRectangle( drawTextBox )`: click one corner, click the opposite one -- the box follows the pointer in between -- and Text Box Properties opens on the box
 * the layer's text defaults make. Clicking the same spot twice, or Escape, ends the tool; after OK or Cancel the tool waits for the next box.
 */
function startTextBoxTool(ctx: SweepCtx): void {
  const { api, dispatch } = ctx;
  const layer = () => activeLayerName(api.getState().activeLayer, "F.SilkS");
  const end = () => setToolPreview(null);
  const first = (): void => {
    picker.start({
      kind: "point",
      prompt: "Text Box: click one corner of the box",
      onPoint: (a) => {
        setToolPreview({ kind: "rect", from: [a.x, a.y], layer: layer() });
        return {
          kind: "point",
          prompt: "Text Box: click the opposite corner",
          onPoint: (b) => {
            end();
            const board = api.getState().board;
            if (!board || (a.x === b.x && a.y === b.y)) return false;
            openTextBoxDialog({ id: null, box: newTextBox(board, layer(), [a.x, a.y], [b.x, b.y]), onClose: first });
            return false;
          },
          onCancel: end,
        };
      },
      onCancel: end,
    });
  };
  dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
  first();
}
