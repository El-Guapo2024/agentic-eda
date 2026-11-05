// The Properties dialogs of the Footprint Editor's dimensions, zones, text boxes, barcodes, tables and reference images (UI-LEFT F2): the board's own dialogs
// (`FOOTPRINT_EDIT_FRAME::OnEditItemRequest`, pcbnew/footprint_editor_utils.cpp at KiCad 8303b2ad, opens the same `DIALOG_DIMENSION_PROPERTIES`, `InvokeRuleAreaEditor` /
// `InvokeCopperZonesEditor` / `InvokeNonCopperZonesEditor`, `DIALOG_TEXTBOX_PROPERTIES`, `DIALOG_BARCODE_PROPERTIES`, `DIALOG_TABLE_PROPERTIES` and
// `DIALOG_REFERENCE_IMAGE_PROPERTIES` as the board editor's), each hosted on the open footprint:
//   - opened by a double click or E on the item (`itemDialog`), OK replaces it with one `fp_item set`, one undo step of this editor;
//   - opened by a drawing tool on the item it has just made (`pendingItem`, components/footprint/fpItemTools.ts): OK adds it with one `fp_item add` and selects it
//     (`commit.Push( _( "Draw Text Box" ) )` ... `selectItem`), Cancel drops it, and the tool waits for its next click.
// The items of a library footprint keep no lock and their zones no net (`eda_model::fp_items`), so these dialogs have no Locked box and the zone dialog no net to pick;
// the Placement tab of a rule area offers what the footprint editor's board has, nothing.
import { useFpApi, useFpDispatch, useFpState, type FootprintEditorState } from "../../state/footprintEditorStore";
import { useStudioState } from "../../state/store";
import { DimensionPropertiesForm, type DimensionHost } from "../DimensionPropertiesDialog";
import { DEFAULT_FORM_SETTINGS, ZoneForm, type ZoneFormSettings, type ZoneHost } from "../ZoneDialog";
import { TextBoxForm, RefImagePropertiesForm, type TextBoxHost, type RefImageHost } from "../PcbItemDialogs";
import { BarcodeForm, type BarcodeHost } from "../BarcodeDialog";
import { TablePropertiesForm, type TableHost } from "../TableDialogs";
import { FP_EDITOR_LAYERS } from "../../kicad-port/fpDrawDefaults";
import { fpNewTable, fpPatchTable, fpZoneWithSettings, type FpTable } from "../../kicad-port/fpDrawTools";
import { tablePatchOf } from "../../kicad-port/tables";
import { lineThicknessNm } from "../../kicad-port/layerDefaults";
import { isCopper } from "../../kicad-port/layerPresets";
import { NO_CHOICES } from "../../kicad-port/multichannel";
import { dataUrlOf } from "../../actions/imageFiles";
import type { CmdDimension, CmdZone } from "../../api/types";
import type { CmdBarcode, CmdRefImage, CmdTextBox } from "../../api/pcbItemTypes";
import type { FpItemWire } from "../../api/fpItemTypes";

const layers = [...FP_EDITOR_LAYERS];
const layerName = (l: string) => l;
const items = (st: FootprintEditorState) => st.footprint?.items ?? {};
const display = (st: FootprintEditorState) => st.footprint?.display;

/** The dialog open on an item of `kind` (`itemDialog`), or null. */
function editingId(st: FootprintEditorState, kind: string): string | null {
  return st.itemDialog?.kind === kind ? st.itemDialog.id : null;
}

/** The two ways a dialog ends: OK on an item there is (`fp_item set`) or on a tool's new one (`fp_item add`, then the new item selected). */
function useCommit() {
  const api = useFpApi();
  const dispatch = useFpDispatch();
  return async (id: string | null, item: FpItemWire): Promise<boolean> => {
    if (id) return api.setItem(id, item);
    const made = await api.addItem(item);
    if (made) dispatch({ type: "SET_SELECTION", refs: [made] });
    return made !== undefined;
  };
}

function useClose(kind: string) {
  const dispatch = useFpDispatch();
  const st = useFpState();
  return () => {
    if (st.itemDialog?.kind === kind) dispatch({ type: "SET_ITEM_DIALOG", dialog: null });
    if (st.pendingItem) dispatch({ type: "SET_PENDING_ITEM", item: null });
  };
}

/** Dimension Properties on a dimension of the footprint (`case PCB_DIM_*_T: DIALOG_DIMENSION_PROPERTIES`). */
export function FpDimensionDialog() {
  const st = useFpState();
  const api = useFpApi();
  const units = useStudioState().units;
  const commit = useCommit();
  const close = useClose("dimension");
  const id = editingId(st, "dimension");
  const dim = id ? display(st)?.dimensions.find((d) => d.id === id) : undefined;
  const ir = id ? (items(st).dimensions ?? []).find((d) => d.id === id) : undefined;
  if (!id || !dim || !ir) return null;
  const host: DimensionHost = {
    dim,
    base: () => JSON.parse(JSON.stringify(ir)) as CmdDimension,
    units,
    layers,
    layerName,
    submit: (cmd) => commit(id, { dimension: cmd }),
    remove: async () => {
      await api.deleteItems([id]);
      return true;
    },
    close,
  };
  return <DimensionPropertiesForm host={host} />;
}

/** The IR zone's settings as the form holds them. */
function settingsOf(z: CmdZone): ZoneFormSettings {
  const out = { ...DEFAULT_FORM_SETTINGS } as Record<string, unknown>;
  for (const k of Object.keys(DEFAULT_FORM_SETTINGS)) if ((z as unknown as Record<string, unknown>)[k] !== undefined) out[k] = (z as unknown as Record<string, unknown>)[k];
  for (const k of ["smoothing", "corner_radius", "placement"]) if ((z as unknown as Record<string, unknown>)[k] !== undefined) out[k] = (z as unknown as Record<string, unknown>)[k];
  return out as unknown as ZoneFormSettings;
}

/**
 * Rule Area Properties (or, for a zone that is no rule area, the copper or non-copper zone dialog: `if( zone->GetIsRuleArea() ) InvokeRuleAreaEditor .. else if(
 * zone->IsOnCopperLayer() ) InvokeCopperZonesEditor .. else InvokeNonCopperZonesEditor`) on a zone of the footprint, or on the rule area Draw Rule Area has just closed.
 */
export function FpZoneDialog() {
  const st = useFpState();
  const api = useFpApi();
  const units = useStudioState().units;
  const commit = useCommit();
  const close = useClose("zone");
  const id = editingId(st, "zone");
  const pending = st.pendingItem?.kind === "zone" ? st.pendingItem.zone : null;
  const zone = id ? ((items(st).zones ?? []).find((z) => z.id === id) ?? null) : pending;
  if (!zone) return null;
  const shown = id ? display(st)?.zones.find((z) => z.id === id) : undefined;
  const zoneLayers = shown?.layers && shown.layers.length > 0 ? [...shown.layers] : [zone.layer];
  const host: ZoneHost = {
    openKey: id ?? `new:${JSON.stringify(zone.outline[0] ?? null)}:${zone.outline.length}`,
    init: () => ({ net: "", layers: zoneLayers, name: (zone as CmdZone & { name?: string }).name ?? "", locked: false, settings: settingsOf(zone) }),
    placement: zone.placement,
    editing: !!id,
    duplicate: false,
    convert: false,
    nets: [],
    copperLayers: layers.filter(isCopper),
    optionalLayers: layers.filter((l) => !isCopper(l)),
    layerName,
    units,
    lockable: false,
    outlinePoints: id ? null : zone.outline.length,
    placementSources: async () => NO_CHOICES,
    submit: async (r) => {
      if (await commit(id, { zone: fpZoneWithSettings(zone, r.form, r.layers, r.name) })) close();
    },
    remove: id
      ? () => {
          void api.deleteItems([id]);
          close();
        }
      : null,
    close,
  };
  return <ZoneForm host={host} />;
}

/** Text Box Properties on a text box of the footprint (`case PCB_TEXTBOX_T: ShowTextBoxPropertiesDialog`) or on the box Draw Text Box has just drawn. */
export function FpTextBoxDialog() {
  const st = useFpState();
  const dispatch = useFpDispatch();
  const commit = useCommit();
  const close = useClose("textbox");
  const id = editingId(st, "textbox");
  const pending = st.pendingItem?.kind === "textbox" ? st.pendingItem.box : null;
  const box = id ? (display(st)?.text_boxes.find((t) => t.id === id) ?? null) : pending;
  if (!box) return null;
  const host: TextBoxHost = {
    openKey: id ?? pending,
    box,
    locked: null,
    layers,
    layerName,
    toast: (message) => dispatch({ type: "TOAST", message, kind: "info" }),
    submit: async (cmd: CmdTextBox) => {
      const { id: _drop, ...rest } = cmd;
      return (await commit(id, { text_box: rest })) ? null : "The footprint refused the text box.";
    },
    close,
  };
  return <TextBoxForm host={host} />;
}

/** Barcode Properties on a barcode of the footprint (`case PCB_BARCODE_T: ShowBarcodePropertiesDialog`) or on the one Add Barcode has just made. */
export function FpBarcodeDialog() {
  const st = useFpState();
  const commit = useCommit();
  const close = useClose("barcode");
  const id = editingId(st, "barcode");
  const pending = st.pendingItem?.kind === "barcode" ? st.pendingItem.barcode : null;
  const barcode: CmdBarcode | null = id ? ((items(st).barcodes ?? []).find((b) => b.id === id) ?? null) : pending;
  if (!barcode) return null;
  const host: BarcodeHost = {
    openKey: id ?? pending,
    barcode,
    locked: null,
    layers,
    layerName,
    submit: async (b) => ((await commit(id, { barcode: b })) ? null : "The footprint refused the barcode."),
    close,
  };
  return <BarcodeForm host={host} />;
}

/** Table Properties on a table of the footprint (`case PCB_TABLE_T: DIALOG_TABLE_PROPERTIES`) or on the one Draw Table has just sized. */
export function FpTableDialog() {
  const st = useFpState();
  const units = useStudioState().units;
  const commit = useCommit();
  const close = useClose("table");
  const id = editingId(st, "table");
  const drawn = st.pendingItem?.kind === "table" ? st.pendingItem.drawn : undefined;
  const table = id ? display(st)?.tables.find((t) => t.id === id) : undefined;
  const ir = id ? ((items(st).tables ?? []) as FpTable[]).find((t) => t.id === id) : undefined;
  if (id ? !table || !ir : !drawn) return null;
  const host: TableHost = {
    openKey: id ? `edit:${id}` : `draw:${JSON.stringify(drawn)}`,
    table,
    drawn: drawn ? { ...drawn, at: [drawn.at[0], drawn.at[1]] } : undefined,
    sch: false,
    locked: null,
    layers,
    layerName,
    units,
    defaultLine: (layer) => lineThicknessNm(undefined, layer),
    submit: async (v) => {
      // `DrawTable` / the board's `TableCmd::Draw`: the grid on the dialog's layer, then what the dialog set; an edit: the patch, then the layer (`set_table_in`)
      const made = drawn ? fpPatchTable(fpNewTable({ ...drawn, layer: v.layer }), tablePatchOf({ cols: drawn.cols, layer: drawn.layer }, v)) : ir && table ? fpPatchTable(ir, tablePatchOf(table, v)) : null;
      if (!made) return false;
      if (!drawn) made.layer = v.layer;
      return commit(id, { table: made });
    },
    close,
  };
  return <TablePropertiesForm host={host} />;
}

/** Reference Image Properties on an image of the footprint (`case PCB_REFERENCE_IMAGE_T: ShowReferenceImagePropertiesDialog`). Its pixels are the IR's, shown from its own data. */
export function FpRefImageDialog() {
  const st = useFpState();
  const commit = useCommit();
  const close = useClose("image");
  const id = editingId(st, "image");
  const image = id ? display(st)?.images.find((i) => i.id === id) : undefined;
  const ir: CmdRefImage | undefined = id ? (items(st).images ?? []).find((i) => i.id === id) : undefined;
  if (!id || !image || !ir) return null;
  const host: RefImageHost = {
    openKey: id,
    image,
    previewUrl: dataUrlOf({ name: id, data: ir.data }),
    locked: null,
    layers,
    layerName,
    submit: async ({ layer, at, scalePpm, grey }) => ((await commit(id, { image: { ...ir, layer, at, scale_ppm: scalePpm, data: grey ?? ir.data } })) ? null : "The footprint refused the image."),
    close,
  };
  return <RefImagePropertiesForm host={host} />;
}
