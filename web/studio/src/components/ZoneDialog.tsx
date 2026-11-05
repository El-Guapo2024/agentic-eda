// pcbnew.InteractiveDrawing.zone ("Draw Filled Zones") and "E"/double-click
// on an existing zone -- port of pcbnew/dialogs/panel_zone_properties.cpp
// (dialog_copper_zones.cpp's one real panel) and of
// dialog_non_copper_zones_properties.cpp, covering every `ZONE_SETTINGS`
// field the IR actually models (crates/model/src/ir.rs `Zone`).
//
// Which dialog a zone gets follows its layers (zone_create_helper.cpp
// `createNewZone`): a zone on a copper layer is a copper pour (`InvokeCopper
// ZonesEditor`: net, clearance, pad connections, islands, corner smoothing,
// fill), one on no copper layer is a non-copper zone (`InvokeNonCopper
// ZonesEditor`: silkscreen, mask, paste and user layers -- no net, no
// clearances, only the minimum width, the corner smoothing and the fill
// style), and a rule area has its own panel (`InvokeRuleAreaEditor`). Source
// swaps panels on `IsRuleArea()` the same way, so this is one dialog with
// three bodies. Every kind has the layer list (`m_layers`: the copper layers
// for a pour, the technical and user layers for a non-copper zone, both for
// a rule area; one or more can be checked), the name and "Locked".
//
// Every kind has "Outline display:" (Line / Hatched / Fully hatched, `m_OutlineDisplayCtrl`) and "Outline hatch pitch:" (`m_outlineHatchPitch`, 0.1 to 2 mm), how
// the outline is drawn (kicad-port/zoneHatch.ts, `ZONE::HatchBorder`); a copper zone's hatched fill has "Hatch offset overrides:" (`m_layerSpecificOverrides`,
// `LAYER_PROPERTIES_GRID_TABLE`: a layer of the zone and its X and Y offset each), the per-zone counterpart of Board Setup > Zone Hatch Offsets.
//
// KiCad asks for a new zone's settings *before* the outline is drawn: Add Filled Zone and Add Rule Area open this dialog at the first corner
// (`ZONE_CREATE_HELPER::OnFirstPoint` -> `createNewZone`, state.pcbx.zoneCreate, kicad-port/pcbDrawZone.ts); its OK locks the corner in and the zone being drawn
// takes these settings when it is finished (`zoneInfo.ExportSetting( *newZone )`), Cancel draws nothing and leaves the tool armed. Properties (E) during the draw
// opens it again on those settings. Create Zone from Selection still opens it on an outline the conversion made (state.zonePending), and its OK adds the zones
// (`api.addZone`: `add_zone` with the settings in one undo step). The same dialog, in "edit" mode (state.zoneEditId), re-opens on an existing zone's current
// settings and commits everything through `edit_zone` alone (outline unchanged).
import { useCallback, useEffect, useMemo, useState } from "react";
import type { Cmd, FillMode, IslandRemovalMode, PadConnection, RuleAreaFields, ZoneSettingsFields, ZoneSmoothing } from "../api/types";
import { DEFAULT_RULE_AREA_SETTINGS, DEFAULT_ZONE_SETTINGS, useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { nmFrom, nmTo } from "../state/units";
import { fetchPlacementSources } from "../api/multichannelClient";
import { chooseMode, NO_CHOICES, placementFormOf, placementOf, placementOptions, placementSelection, type PlacementChoices, type PlacementForm } from "../kicad-port/multichannel";
import { duplicatedZoneOutline } from "../kicad-port/boardControl";
import { enabledOptionalLayers, layerDisplayName } from "../kicad-port/boardLayers";
import { activeLayerName, isCopper } from "../kicad-port/layerPresets";
import { postCarry } from "../api/client";
import { NO_NET_ZONE_WARNING, convertZoneTitle, orderedLayerSet, zoneDialogKind, zoneDialogNet, zoneLayerChoices, zoneLayers } from "../kicad-port/zoneLayers";
import { convertPolys } from "../api/client";
import { deleteCmd } from "../kicad-port/pcbConvert";
import { rememberZoneConversion } from "../actions/pcbConvertSweep";
import { LengthRow, lengthText, useEscape, useLengthField } from "./pcbDialogKit";
import { stringFromValue } from "../kicad-port/fpFields";
import { DEFAULT_ZONE_BORDER_STYLE, ZONE_BORDER_HATCH_DIST_NM, ZONE_BORDER_STYLE_CHOICES, borderHatchPitchError } from "../kicad-port/zoneHatch";
import { ALL_ZONE_LAYERS_OVERRIDDEN, hatchOffsetRows, hatchOffsetsOf, nextOffsetLayer, sameHatchOffsets, type HatchOffsetRow } from "../kicad-port/zoneHatchOffsets";
import type { ZoneBorderStyle } from "../api/types";

type ZoneFormSettings = ZoneSettingsFields & RuleAreaFields;
const DEFAULT_FORM_SETTINGS: ZoneFormSettings = { ...DEFAULT_ZONE_SETTINGS, ...DEFAULT_RULE_AREA_SETTINGS };

/** `ZONE::GetDoNotAllow*` -- task item 3's five keepout restrictions. */
const KEEPOUT_OPTIONS: { key: Exclude<keyof RuleAreaFields, "is_rule_area" | "placement">; label: string }[] = [
  { key: "keepout_tracks", label: "Tracks" },
  { key: "keepout_vias", label: "Vias" },
  { key: "keepout_pads", label: "Pads" },
  { key: "keepout_copper_pour", label: "Copper pours" },
  { key: "keepout_footprints", label: "Footprints" },
];

const PAD_CONNECTION_OPTIONS: { value: PadConnection; label: string }[] = [
  { value: "Full", label: "Solid" },
  { value: "Thermal", label: "Thermal reliefs" },
  { value: "ThtThermal", label: "Thermal reliefs for PTH only" },
  { value: "None", label: "None" },
];

/** panel_zone_properties.cpp's "Corner smoothing" (`m_cornerSmoothingChoice`). */
const SMOOTHING_OPTIONS: { value: ZoneSmoothing; label: string }[] = [
  { value: "none", label: "None" },
  { value: "chamfer", label: "Chamfer" },
  { value: "fillet", label: "Fillet" },
];

const ISLAND_REMOVAL_OPTIONS: { value: IslandRemovalMode; label: string }[] = [
  { value: "Always", label: "Remove islands" },
  { value: "Never", label: "Keep islands" },
  { value: "Area", label: "Remove islands below area limit" },
];

/** A length field bound to one `ZoneSettingsFields` key, shown/edited in the status bar's current unit -- converts to/from µm only at the input boundary, same spirit `MoveExactDialog.tsx` already uses. */
function LengthInput({ valueNm, unit, onChange, disabled, testid }: { valueNm: number; unit: Parameters<typeof nmTo>[1]; onChange: (um: number) => void; disabled?: boolean; testid?: string }) {
  return (
    <input
      data-testid={testid}
      type="number"
      step="any"
      disabled={disabled}
      value={nmTo(valueNm, unit)}
      onChange={(e) => {
        const n = Number(e.target.value);
        if (Number.isFinite(n)) onChange(Math.round(nmFrom(n, unit)));
      }}
      style={{ width: 90 }}
    />
  );
}

const SECTION: React.CSSProperties = { color: "var(--chrome-text-dim)", fontSize: 11, margin: "12px 0 4px", fontWeight: 600 };

export function ZoneDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const units = state.units;

  const addOutline = state.zonePending;
  // A zone being drawn: the dialog at its first corner, or Properties (E) during the draw (kicad-port/pcbDrawZone.ts).
  const create = state.pcbx.zoneCreate ?? null;
  const editId = state.zoneEditId;
  const editZone = editId ? api.zoneById(editId) : undefined;
  // `pcbnew.EditorControl.zoneDuplicate`: the dialog opens on a copy of this zone's settings; OK adds the copy.
  const dupId = state.bcx.duplicateZoneId;
  const dupZone = dupId ? api.zoneById(dupId) : undefined;
  const open = create != null || addOutline != null || (editId != null && editZone != null) || dupZone != null;

  const nets = useMemo(() => {
    const set = new Set<string>();
    for (const p of state.board?.parts ?? []) for (const pad of p.pads ?? []) if (pad.net) set.add(pad.net);
    return [...set].sort();
  }, [state.board]);

  // `null`: a new zone nothing was picked for (it starts on the first net); `""`: on no net (`<no net>`).
  const [net, setNet] = useState<string | null>(null);
  /** The layers checked in the list (`ZONE_SETTINGS::m_Layers`), in the order the board lists them. */
  const [layers, setLayers] = useState<string[]>([]);
  const [name, setName] = useState("");
  const [locked, setLocked] = useState(false);
  const [settings, setSettings] = useState<ZoneFormSettings>(DEFAULT_FORM_SETTINGS);
  /** The Hatched Fill tab's "Hatch offset overrides:" grid, and what it opened with (an unchanged grid is not sent, so a zone keeps its own). */
  const [offsets, setOffsets] = useState<HatchOffsetRow[]>([]);
  const [offsetsAtOpen, setOffsetsAtOpen] = useState<HatchOffsetRow[]>([]);
  const [offsetNote, setOffsetNote] = useState<string | null>(null);
  // "Outline hatch pitch:" as typed, in the display units (`UNIT_BINDER`): validated on OK.
  const pitchField = useLengthField(ZONE_BORDER_HATCH_DIST_NM);
  // The Placement tab of a rule area (`DIALOG_RULE_AREA_PROPERTIES`): the sheets, component classes and groups the board offers, and what is chosen among them.
  const [sources, setSources] = useState<PlacementChoices | null>(null);
  const [placement, setPlacement] = useState<PlacementForm>(() => placementFormOf(undefined, NO_CHOICES));
  // Create Zone / Rule Area from Selection: the dialog's "Conversion Settings" box (`DIALOG_COPPER_ZONE( ..., aConvertSettings )`; round 6 D6-2).
  const convert = addOutline != null ? state.pcbx.zoneConvert : null;
  const [convStrategy, setConvStrategy] = useState<"centerline" | "bounding_hull">("centerline");
  const convGap = useLengthField(0);
  const [convDelete, setConvDelete] = useState(false);

  const copperLayers = state.board?.layers ?? [];
  const optionalLayers = enabledOptionalLayers(state.board?.board_rules?.layer_setup);
  const lockedIds = state.board?.locked;

  // (Re)populate whenever the dialog opens on a *different* target --
  // never on every render, or a mid-edit keystroke would be clobbered by
  // this same effect re-reading the (not yet re-fetched) board.
  useEffect(() => {
    if (!open) return;
    const startFrom = (z: { border_style?: ZoneBorderStyle; border_hatch_pitch?: number; hatch_offsets?: Record<string, { x_nm: number; y_nm: number }> } | null) => {
      const rows = hatchOffsetRows(z?.hatch_offsets);
      setOffsets(rows);
      setOffsetsAtOpen(rows);
      setOffsetNote(null);
      pitchField.setText(lengthText(z?.border_hatch_pitch ?? ZONE_BORDER_HATCH_DIST_NM, units));
    };
    if (create) {
      // `createNewZone`'s `zoneInfo` (or, for Properties during the draw, the settings the zone being drawn holds).
      setNet(create.initial.net);
      setLayers([...create.initial.layers]);
      setName(create.initial.name);
      setLocked(create.initial.locked);
      setSettings({ ...DEFAULT_FORM_SETTINGS, ...create.initial.settings });
      startFrom(create.initial.settings);
    } else if (editZone) {
      setNet(editZone.net);
      setLayers([...zoneLayers(editZone)]);
      setName(editZone.name ?? "");
      setLocked(!!lockedIds?.includes(editZone.id));
      setSettings({ ...editZone });
      startFrom(editZone);
    } else if (dupZone) {
      // `zoneSettings << *oldZone`: the copy starts from the original's net, layers, name, lock and settings.
      setNet(dupZone.net);
      setLayers([...zoneLayers(dupZone)]);
      setName(dupZone.name ?? "");
      setLocked(!!lockedIds?.includes(dupZone.id));
      setSettings({ ...dupZone });
      startFrom(dupZone);
    } else {
      // `createNewZone`: the zone is on the layer the tool was started on -- a technical layer gets the non-copper dialog.
      setNet(null);
      setLayers([activeLayerName(state.activeLayer, copperLayers[0] ?? "F.Cu")]);
      setName("");
      setLocked(false);
      setSettings({ ...DEFAULT_FORM_SETTINGS, is_rule_area: state.nextZoneIsRuleArea });
      startFrom(null);
      dispatch({ type: "SET_NEXT_ZONE_IS_RULE_AREA", value: false });
      if (convert) {
        setConvStrategy(convert.strategy);
        convGap.setText(lengthText(convert.gapNm, units));
        setConvDelete(convert.deleteOriginals);
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, editId, dupId, create]);

  // What the Placement tab offers is read from the board when the dialog opens; the form starts from the zone's own placement once it has them.
  useEffect(() => {
    if (!open) return;
    let live = true;
    setSources(null);
    void fetchPlacementSources()
      .then((r) => {
        if (!live) return;
        const found: PlacementChoices = r.ok ? { sheets: r.sheets, classes: r.classes, groups: r.groups } : NO_CHOICES;
        setSources(found);
        setPlacement(placementFormOf((editZone ?? dupZone)?.placement, found));
      })
      .catch(() => live && setSources(NO_CHOICES));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, editId, dupId]);

  // Escape in the dialog of a zone being drawn is its Cancel (`dialogResult == wxID_CANCEL`): the tool stays armed, so the board's own Escape must not see it.
  const cancelCreate = useCallback(() => dispatch({ type: "PCBX", patch: { zoneCreate: null } }), [dispatch]);
  useEscape(create ? cancelCreate : null);

  if (!open) return null;

  const kind = zoneDialogKind(settings.is_rule_area, layers);
  const choices = zoneLayerChoices(kind, copperLayers, optionalLayers);
  // A zone can hold a layer the list does not offer (a rule area's technical layers once the box is unchecked, a layer the board has since turned off): keep it in view.
  const listed = [...choices, ...layers.filter((l) => !choices.includes(l))];
  // A rule area normally has no net at all (KiCad's net code 0) -- unlike a new copper-pour zone, never defaulted to the first net on the board; a non-copper zone has
  // none; a copper zone on no net stays on none (R5-Z3).
  const effectiveNet = zoneDialogNet(kind, net, nets);
  const set = <K extends keyof ZoneFormSettings>(key: K, value: ZoneFormSettings[K]) => setSettings((s) => ({ ...s, [key]: value }));
  const setRuleArea = (on: boolean) => {
    set("is_rule_area", on);
    // Only a rule area is on copper and technical layers at once: unchecking the box keeps the copper layers, else the technical ones.
    if (!on && layers.some(isCopper) && layers.some((l) => !isCopper(l))) setLayers(layers.filter(isCopper));
  };
  const toggleLayer = (layer: string, on: boolean) => setLayers(orderedLayerSet(on ? [...layers, layer] : layers.filter((l) => l !== layer), listed));

  const close = () => {
    if (create) dispatch({ type: "PCBX", patch: { zoneCreate: null } });
    dispatch({ type: "SET_ZONE_PENDING", outline: null });
    dispatch({ type: "SET_ZONE_EDIT_ID", id: null });
    if (dupZone) dispatch({ type: "BCX", patch: { duplicateZoneId: null } });
    dispatch({ type: "PCBX", patch: { zoneConvert: null } });
  };

  // panel_zone_properties.cpp's AcceptOptions(): the one cross-field check
  // this app's backend also enforces (`ops_bad_zone`) -- caught here too
  // so the dialog can disable OK instead of round-tripping a refusal. A
  // rule area skips every fill-engine check (clearance/min-width/thermal
  // spoke are all unused fields for one, matching source) and doesn't
  // need a net at all; a non-copper zone has no clearances or spokes
  // either. "No layer selected." is both KiCad dialogs' check on the list.
  const noLayer = layers.length === 0;
  const thermalSpokeTooNarrow = kind === "copper" && settings.thermal_spoke_width < settings.min_thickness;
  // (a copper zone on no net is allowed: the panel only warns, `updateInfoBar`)
  const convGapNm = convert && convStrategy === "bounding_hull" ? convGap.parse() : 0;
  // `m_outlineHatchPitch.Validate( ZONE_BORDER_HATCH_MINDIST_MM, ZONE_BORDER_HATCH_MAXDIST_MM )` (all three dialogs).
  const pitchNm = pitchField.parse();
  const pitchError = pitchNm === null ? "Outline hatch pitch is not a length." : borderHatchPitchError(pitchNm, (nm) => stringFromValue(nm, units));
  const canSubmit = convGapNm !== null && !pitchError && !noLayer && (kind === "rule-area" ? true : kind === "non-copper" ? settings.min_thickness >= 0 : !thermalSpokeTooNarrow && settings.clearance >= 0 && settings.min_thickness > 0);
  const firstLayer = layers[0] ?? "F.Cu";

  const submit = async () => {
    if (!canSubmit) return;
    // The zone's net is the form's own; the settings carry the rest (a spread of the zone being edited would bring its old net and layer back over the form's).
    // `AcceptOptions`: the outline display and its pitch; the hatch offset overrides replace the zone's (`props.hatching_offset = std::nullopt` for each, then the
    // grid's rows) -- sent only when the grid changed, so a zone keeps the ones it had.
    const offsetsChanged = !sameHatchOffsets(offsets, offsetsAtOpen);
    const form = {
      ...settings,
      border_style: settings.border_style ?? DEFAULT_ZONE_BORDER_STYLE,
      border_hatch_pitch: pitchNm ?? ZONE_BORDER_HATCH_DIST_NM,
      hatch_offsets: offsetsChanged ? hatchOffsetsOf(offsets, layers) : undefined,
      ...(settings.is_rule_area ? { placement: placementOf(placement) } : {}),
    };
    if (create) {
      // `createNewZone` returns the settings; `OnFirstPoint` then locks the first corner in (`AddPoint`) and clears the selection (`selectionClear`). Properties
      // during the draw (`at` null) only changes what the zone being drawn takes.
      const chosen = { net: effectiveNet, layers, name: settings.is_rule_area ? "" : name, locked, settings: form };
      const draw = state.drawState;
      if (create.at) {
        dispatch({ type: "SET_SELECTION", refs: [] });
        dispatch({ type: "SET_DRAW_STATE", draw: { kind: "zone", pts: [create.at], settings: chosen } });
      } else if (draw?.kind === "zone") {
        dispatch({ type: "SET_DRAW_STATE", draw: { ...draw, settings: chosen } });
      }
      close();
      return;
    }
    if (editZone) {
      const cmds: Cmd[] = [{ op: "edit_zone", ...form, id: editZone.id, net: effectiveNet, layer: firstLayer, layers }];
      if ((editZone.name ?? "") !== name && !settings.is_rule_area) cmds.push({ op: "set_zone_name", id: editZone.id, name });
      if (!!lockedIds?.includes(editZone.id) !== locked) cmds.push({ op: "set_locked", ids: [editZone.id], locked });
      const ok = cmds.length === 1 ? await api.cmd(cmds[0]!) : await api.cmdBatch(cmds);
      if (ok) close();
    } else if (addOutline) {
      const extra = { layers, name: settings.is_rule_area ? "" : name, locked };
      if (convert) {
        // "Create Zone from Selection": `getPolys( m_userSettings )` with what the box holds now (nothing to convert: nothing is done), a zone with the dialog's
        // settings for every outline, then the converted sources go -- all one undo step, as `CreatePolys` pushes one commit (round 5 B5-10): the zones join a
        // step held open, which the removal (or a bare commit) closes.
        const chosen = { strategy: convStrategy, gapNm: convGapNm ?? 0, deleteOriginals: convDelete };
        rememberZoneConversion(chosen);
        const reply = await convertPolys(convert.ids, chosen.strategy, chosen.gapNm);
        if (!reply.ok || (reply.polys ?? reply.rings).length === 0 || !state.board) {
          close();
          return;
        }
        const board = state.board;
        // `polys.emplace_back( SHAPE_POLY_SET( polySet.COutline( ii ) ) ); polys.back().AddHole( .. )`: each outline with its holes (a zone source's are kept).
        const polys = reply.polys ?? reply.rings.map((outline) => ({ outline, holes: [] as [number, number][][] }));
        for (const poly of polys) await api.addZone(effectiveNet, firstLayer, poly.outline, form, { ...extra, holes: poly.holes }, "hold");
        const deleteCmds: Cmd[] = chosen.deleteOriginals ? reply.consumed.map((id) => deleteCmd(board, id)) : [];
        if (deleteCmds.length > 0) await api.cmdBatch(deleteCmds, "commit");
        else await postCarry("commit");
      } else await api.addZone(effectiveNet, firstLayer, addOutline, form, extra);
      close();
    } else if (dupZone) {
      // `ZoneDuplicate`: the copy is unfilled (fills are derived here), has the dialog's settings, and -- on the same layers as the original --
      // is offset a bit so it can be picked.
      const sameLayers = zoneLayers(dupZone).join("|") === layers.join("|");
      await api.addZone(effectiveNet, firstLayer, duplicatedZoneOutline(dupZone.outline, sameLayers), form, { layers, name: settings.is_rule_area ? "" : name, locked });
      close();
    }
  };

  const title = convert
    ? convertZoneTitle(kind)
    : editZone
    ? kind === "rule-area"
      ? "Rule Area Properties"
      : kind === "non-copper"
        ? "Non-copper Zone Properties"
        : "Zone Properties"
    : dupZone
      ? "Zone Properties -- Duplicate Zone onto Layer"
      : kind === "rule-area"
        ? "Rule Area Properties -- New Rule Area"
        : kind === "non-copper"
          ? "Non-copper Zone Properties -- New Zone"
          : "Zone Properties -- New Zone";

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 460 }} data-testid="zone-dialog" data-zone-dialog={kind} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "70vh", overflowY: "auto" }}>
          {convert && (
            <fieldset data-testid="zone-conversion-settings" style={{ marginBottom: 8, border: "1px solid var(--chrome-border, #444)", padding: "4px 8px" }}>
              <legend style={{ fontSize: 11 }}>Conversion Settings</legend>
              <label className="filter-row" style={{ display: "block" }}>
                <input type="radio" name="zone-convert-strategy" checked={convStrategy === "centerline"} onChange={() => setConvStrategy("centerline")} /> Use centerlines
              </label>
              <label className="filter-row" style={{ display: "block" }}>
                <input type="radio" name="zone-convert-strategy" checked={convStrategy === "bounding_hull"} onChange={() => setConvStrategy("bounding_hull")} /> Create bounding hull
              </label>
              <div style={{ marginLeft: 22 }}>
                <LengthRow label="Gap:" field={convGap} disabled={convStrategy !== "bounding_hull"} />
              </div>
              <label className="filter-row" style={{ display: "block" }}>
                <input type="checkbox" checked={convDelete} onChange={(e) => setConvDelete(e.target.checked)} /> Delete source objects after conversion
              </label>
            </fieldset>
          )}
          <label className="filter-row" style={{ display: "block", marginBottom: 8 }} title="ZONE::GetIsRuleArea() -- a keepout instead of a copper pour">
            <input type="checkbox" data-testid="zone-rule-area" checked={settings.is_rule_area} onChange={(e) => setRuleArea(e.target.checked)} /> Rule area (keepout)
          </label>

          <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
            {!settings.is_rule_area && (
              <>
                <span>Name</span>
                <input value={name} onChange={(e) => setName(e.target.value)} placeholder="(unnamed)" data-testid="zone-name" />
              </>
            )}
            {kind === "copper" && (
              <>
                <span>Net</span>
                <select value={effectiveNet} onChange={(e) => setNet(e.target.value)} data-testid="zone-net">
                  <option value="">{"<no net>"}</option>
                  {nets.map((n) => (
                    <option key={n} value={n}>
                      {n}
                    </option>
                  ))}
                </select>
              </>
            )}
            <span>Layers</span>
            <div role="group" aria-label="Zone layers" style={{ maxHeight: 130, overflowY: "auto", border: "1px solid var(--chrome-border, #444)", padding: "2px 6px" }}>
              {listed.map((l) => (
                <label key={l} className="filter-row" style={{ display: "block" }} data-zone-layer={l}>
                  <input type="checkbox" data-testid={`zone-layer-${l}`} checked={layers.includes(l)} onChange={(e) => toggleLayer(l, e.target.checked)} /> {layerDisplayName(state.board?.board_rules?.layer_setup, l)}
                </label>
              ))}
            </div>
          </div>
          {noLayer && <p style={{ color: "#ef5b5b", fontSize: 11, margin: "4px 0 0" }}>No layer selected.</p>}
          {kind === "copper" && effectiveNet === "" && (
            <p style={{ color: "#e0a030", fontSize: 11, margin: "4px 0 0" }} data-testid="zone-no-net-warning">
              {NO_NET_ZONE_WARNING}
            </p>
          )}
          <label className="filter-row" style={{ display: "block", marginTop: 8 }} title="BOARD_ITEM::IsLocked() -- a locked zone is not moved or edited by the selection tools">
            <input type="checkbox" checked={locked} onChange={(e) => setLocked(e.target.checked)} data-testid="zone-locked" /> Locked
          </label>

          {kind === "rule-area" ? (
            <>
              <p style={SECTION}>Keep out</p>
              {KEEPOUT_OPTIONS.map((o) => (
                <label key={o.key} className="filter-row" style={{ display: "block" }}>
                  <input type="checkbox" checked={settings[o.key]} onChange={(e) => set(o.key, e.target.checked)} /> {o.label}
                </label>
              ))}
              <p style={SECTION}>Placement</p>
              <div data-testid="zone-placement" role="radiogroup" aria-label="Placement">
                <label className="filter-row" style={{ display: "block" }}>
                  <input type="radio" name="zone-placement" data-testid="zone-placement-none" checked={placement.mode === "none"} onChange={() => setPlacement((f) => chooseMode(f, "none"))} /> No placement
                </label>
                {(
                  [
                    ["sheetname", "Place items from sheet:"],
                    ["component_class", "Place items matching component class:"],
                    ["group", "Place items in group:"],
                  ] as const
                ).map(([type, label]) => (
                  <div key={type} style={{ display: "flex", gap: 8, alignItems: "center", margin: "4px 0" }}>
                    <label className="filter-row" style={{ flex: "0 0 auto" }}>
                      <input type="radio" name="zone-placement" data-testid={`zone-placement-${type}`} checked={placement.mode === type} onChange={() => setPlacement((f) => chooseMode(f, type))} /> {label}
                    </label>
                    <select
                      data-testid={`zone-placement-source-${type}`}
                      value={placementSelection(placement, type)}
                      style={{ flex: 1, minWidth: 0 }}
                      onChange={(e) => {
                        const value = e.target.value;
                        setPlacement((f) => ({ ...f, [type === "sheetname" ? "sheet" : type]: value }));
                      }}
                    >
                      {placementOptions(placement, sources ?? NO_CHOICES, type).map((o) => (
                        <option key={`${type}:${o.value}`} value={o.value}>
                          {o.label}
                        </option>
                      ))}
                    </select>
                  </div>
                ))}
              </div>
            </>
          ) : (
            <>
              <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr", marginTop: 8 }}>
                {kind === "copper" && (
                  <>
                    <span>Clearance</span>
                    <span>
                      <LengthInput valueNm={settings.clearance} unit={units} onChange={(v) => set("clearance", v)} /> {units}
                    </span>
                  </>
                )}
                <span>Minimum width</span>
                <span>
                  <LengthInput valueNm={settings.min_thickness} unit={units} onChange={(v) => set("min_thickness", v)} testid="zone-min-width" /> {units}
                </span>
                {kind === "copper" && (
                  <>
                    <span>Priority</span>
                    <span>
                      <input type="number" step={1} min={0} value={settings.priority} onChange={(e) => set("priority", Math.max(0, Math.round(Number(e.target.value) || 0)))} style={{ width: 90 }} />
                    </span>
                  </>
                )}
              </div>

              {kind === "copper" && (
                <>
                  <p style={SECTION}>Pad connections</p>
                  <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
                    <span>Connection</span>
                    <select value={settings.pad_connection} onChange={(e) => set("pad_connection", e.target.value as PadConnection)}>
                      {PAD_CONNECTION_OPTIONS.map((o) => (
                        <option key={o.value} value={o.value}>
                          {o.label}
                        </option>
                      ))}
                    </select>
                    {/* panel_zone_properties.cpp's own comment: thermal gap/spoke width are never disabled by the connection choice above -- a per-pad override can still need them even when the zone itself is Full/None. */}
                    <span>Thermal relief gap</span>
                    <span>
                      <LengthInput valueNm={settings.thermal_gap} unit={units} onChange={(v) => set("thermal_gap", v)} /> {units}
                    </span>
                    <span>Thermal spoke width</span>
                    <span>
                      <LengthInput valueNm={settings.thermal_spoke_width} unit={units} onChange={(v) => set("thermal_spoke_width", v)} /> {units}
                      {thermalSpokeTooNarrow && <span style={{ color: "#ef5b5b", marginLeft: 8 }}>cannot be smaller than minimum width</span>}
                    </span>
                  </div>

                  <p style={SECTION}>Islands</p>
                  <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
                    <span>Remove islands</span>
                    <select value={settings.island_removal_mode} onChange={(e) => set("island_removal_mode", e.target.value as IslandRemovalMode)}>
                      {ISLAND_REMOVAL_OPTIONS.map((o) => (
                        <option key={o.value} value={o.value}>
                          {o.label}
                        </option>
                      ))}
                    </select>
                    {settings.island_removal_mode === "Area" && (
                      <>
                        <span>Minimum island area</span>
                        <span>
                          <input
                            type="number"
                            step="any"
                            value={settings.min_island_area / 1_000_000}
                            onChange={(e) => {
                              const n = Number(e.target.value);
                              if (Number.isFinite(n)) set("min_island_area", Math.round(n * 1_000_000));
                            }}
                            style={{ width: 90 }}
                          />{" "}
                          mm&sup2;
                        </span>
                      </>
                    )}
                  </div>
                </>
              )}

              <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr", marginTop: 12 }}>
                <span>Corner smoothing</span>
                <select
                  data-testid="zone-smoothing"
                  value={settings.smoothing ?? "none"}
                  // `OnCornerSmoothingSelection` hides the radius for None and `AcceptOptions` stores 0 for it.
                  onChange={(e) => setSettings((s) => ({ ...s, smoothing: e.target.value as ZoneSmoothing, corner_radius: e.target.value === "none" ? 0 : s.corner_radius ?? 0 }))}
                  title="ZONE::BuildSmoothedPoly: the outline's corners are chamfered or filleted before the zone is filled"
                >
                  {SMOOTHING_OPTIONS.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
                {(settings.smoothing ?? "none") !== "none" && (
                  <>
                    {/* The non-copper dialog names it by the shape (`OnUpdateUI`: "Chamfer distance" / "Fillet radius"). */}
                    <span>{kind === "non-copper" ? (settings.smoothing === "chamfer" ? "Chamfer distance" : "Fillet radius") : "Radius"}</span>
                    <span>
                      <LengthInput valueNm={settings.corner_radius ?? 0} unit={units} onChange={(v) => set("corner_radius", Math.max(0, v))} /> {units}
                    </span>
                  </>
                )}
              </div>

              <p style={SECTION}>Fill</p>
              <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
                <span>Fill type</span>
                <select value={settings.fill_mode} onChange={(e) => set("fill_mode", e.target.value as FillMode)} data-testid="zone-fill-type">
                  <option value="Polygons">Solid fill</option>
                  <option value="HatchPattern">Hatch pattern</option>
                </select>
                {settings.fill_mode === "HatchPattern" && (
                  <>
                    <span>Hatch width</span>
                    <span>
                      <LengthInput valueNm={settings.hatch_thickness} unit={units} onChange={(v) => set("hatch_thickness", v)} /> {units}
                    </span>
                    <span>Hatch gap</span>
                    <span>
                      <LengthInput valueNm={settings.hatch_gap} unit={units} onChange={(v) => set("hatch_gap", v)} /> {units}
                    </span>
                    <span>Orientation</span>
                    <span>
                      <input
                        type="number"
                        step="any"
                        value={settings.hatch_orientation_mdeg / 1000}
                        onChange={(e) => {
                          const n = Number(e.target.value);
                          if (Number.isFinite(n)) set("hatch_orientation_mdeg", Math.round(n * 1000));
                        }}
                        style={{ width: 90 }}
                      />
                      °
                    </span>
                    <span>Smoothing level</span>
                    <span>
                      <input
                        type="number"
                        step={1}
                        min={0}
                        max={3}
                        value={settings.hatch_smoothing_level}
                        onChange={(e) => set("hatch_smoothing_level", Math.max(0, Math.min(3, Math.round(Number(e.target.value) || 0))))}
                        style={{ width: 90 }}
                      />
                    </span>
                    <span>Smoothing value</span>
                    <span>
                      <input
                        type="number"
                        step="any"
                        min={0}
                        max={1}
                        value={settings.hatch_smoothing_value}
                        onChange={(e) => {
                          const n = Number(e.target.value);
                          if (Number.isFinite(n)) set("hatch_smoothing_value", Math.max(0, Math.min(1, n)));
                        }}
                        style={{ width: 90 }}
                      />
                    </span>
                  </>
                )}
              </div>
              {kind === "copper" && (
                // "Hatch offset overrides:" (`m_layerSpecificOverrides`), enabled with "Hatched fill" only (`onHatched`); the rows are kept while it is off.
                <div data-testid="zone-hatch-offsets" style={{ marginTop: 8, opacity: settings.fill_mode === "HatchPattern" ? 1 : 0.5 }}>
                  <p style={{ ...SECTION, marginTop: 4 }}>Hatch offset overrides:</p>
                  <table style={{ width: "100%", fontSize: 12, borderCollapse: "collapse" }}>
                    <thead>
                      <tr style={{ textAlign: "left" }}>
                        <th>Layer</th>
                        <th>Offset X</th>
                        <th>Offset Y</th>
                        <th />
                      </tr>
                    </thead>
                    <tbody>
                      {offsets.map((r, i) => (
                        <tr key={`${r.layer}-${i}`} data-hatch-offset-layer={r.layer}>
                          <td>
                            <select
                              disabled={settings.fill_mode !== "HatchPattern"}
                              value={r.layer}
                              onChange={(e) => setOffsets((rows) => rows.map((q, j) => (j === i ? { ...q, layer: e.target.value } : q)))}
                            >
                              {[...new Set([r.layer, ...layers.filter(isCopper)])].map((l) => (
                                <option key={l} value={l}>
                                  {layerDisplayName(state.board?.board_rules?.layer_setup, l)}
                                </option>
                              ))}
                            </select>
                          </td>
                          <td>
                            <LengthInput valueNm={r.x_nm} unit={units} disabled={settings.fill_mode !== "HatchPattern"} onChange={(v) => setOffsets((rows) => rows.map((q, j) => (j === i ? { ...q, x_nm: v } : q)))} testid={`zone-hatch-offset-x-${r.layer}`} />
                          </td>
                          <td>
                            <LengthInput valueNm={r.y_nm} unit={units} disabled={settings.fill_mode !== "HatchPattern"} onChange={(v) => setOffsets((rows) => rows.map((q, j) => (j === i ? { ...q, y_nm: v } : q)))} testid={`zone-hatch-offset-y-${r.layer}`} />
                          </td>
                          <td>
                            <button disabled={settings.fill_mode !== "HatchPattern"} title="Delete row" onClick={() => setOffsets((rows) => rows.filter((_, j) => j !== i))}>
                              ✕
                            </button>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  <button
                    disabled={settings.fill_mode !== "HatchPattern"}
                    data-testid="zone-hatch-offset-add"
                    title="Add a layer override"
                    onClick={() => {
                      const layer = nextOffsetLayer(offsets, layers);
                      if (!layer) {
                        setOffsetNote(ALL_ZONE_LAYERS_OVERRIDDEN);
                        return;
                      }
                      setOffsetNote(null);
                      setOffsets((rows) => [...rows, { layer, x_nm: 0, y_nm: 0 }]);
                    }}
                  >
                    +
                  </button>
                  {offsetNote && <span style={{ color: "#e0a030", fontSize: 11, marginLeft: 8 }}>{offsetNote}</span>}
                </div>
              )}
            </>
          )}

          {/* "Outline display:" / "Outline style:" and "Outline hatch pitch:" (`m_OutlineDisplayCtrl`, `m_outlineHatchPitch`): the outline as `ZONE::HatchBorder` draws it. */}
          <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr", marginTop: 12 }}>
            <span>{kind === "non-copper" ? "Outline style:" : "Outline display:"}</span>
            <select data-testid="zone-outline-display" value={settings.border_style ?? DEFAULT_ZONE_BORDER_STYLE} onChange={(e) => set("border_style", e.target.value as ZoneBorderStyle)}>
              {ZONE_BORDER_STYLE_CHOICES.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
            <span>Outline hatch pitch:</span>
            <span title="Distance between parallel lines used for hatching the area">
              <input data-testid="zone-outline-hatch-pitch" value={pitchField.text} onChange={(e) => pitchField.setText(e.target.value)} style={{ width: 90 }} /> {units}
            </span>
          </div>
          {pitchError && (
            <p style={{ color: "#ef5b5b", fontSize: 11, margin: "4px 0 0" }} data-testid="zone-pitch-error">
              {pitchError}
            </p>
          )}

          {addOutline && <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>{addOutline.length} point outline.</p>}
        </div>
        <div className="dialog-footer">
          {editZone && (
            <button
              onClick={() => {
                api.cmd({ op: "delete_zone", id: editZone.id });
                close();
              }}
            >
              {settings.is_rule_area ? "Delete Rule Area" : "Delete Zone"}
            </button>
          )}
          <button onClick={close} data-testid="zone-cancel">
            Cancel
          </button>
          <button className="primary" disabled={!canSubmit} onClick={submit} data-testid="zone-ok">
            {editZone || convert || create ? "OK" : settings.is_rule_area ? "Add Rule Area" : "Add Zone"}
          </button>
        </div>
      </div>
    </div>
  );
}
