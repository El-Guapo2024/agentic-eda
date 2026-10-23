// The dialogs of KiCad 10's board items that the 8303b2ad source predates (ported from KiCad 25ec2335, the commit of the box's kicad-cli).
//
// Grid Properties: `DIALOG_GRID_ITEM_PROPERTIES` (pcbnew/dialogs/dialog_grid_item_properties.cpp and its _base). The grid's type (Cartesian or Polar); for a cartesian grid three
// pages that all edit the one rectangle -- By Corners, By Corner and Size, By Center and Size -- in world coordinates on the turned outline, each page following the others as one
// is typed in; for a polar grid the Center and Radius page with its angular extent; the orientation; the spacing (Radius spacing and Angular spacing on a polar grid, the values
// each type had kept when the type is switched); the major tick interval and the priority; what the grid affects (Cursor, Routing, Placement); and the lock. OK sends one
// `edit_grid_item` ("Edit Grid Item Properties") and the lock, one undo step.
import { useEffect, useState } from "react";
import { applyCommands } from "../actions/pcbSweepKit";
import type { BoardGridItem } from "../api/pcbItemTypes";
import type { Cmd } from "../api/types";
import { gridDialogCorners, gridFromCentreSize, gridFromCornerSize, gridFromCorners, gridItemCmd, normalizedGridItem } from "../kicad-port/gridItem";
import { closeGridItemDialog, closeViaStitchDialog, useKicad10Dialogs } from "../state/kicad10Dialogs";
import type { BoardViaStitch } from "../api/pcbItemTypes";
import { fetchState } from "../api/client";
import { pcbContext } from "../kicad-port/pcbProperties";
import { defaultPitchUm, layoutOf, modeOf, MODE_CHOICES, movedStitch, PATTERN_CHOICES, stitchPosition, stitchPreview, viaStitchCmd, withLayoutAndMode, type StitchLayout, type StitchMode } from "../kicad-port/viaStitch";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { umFrom, umTo, type LengthUnit } from "../state/units";
import { layerColor as layerColorOf } from "./canvas/layers";

export function Kicad10ItemDialogs() {
  return (
    <>
      <GridItemDialog />
      <ViaStitchDialog />
    </>
  );
}

type Page = "corners" | "cornerSize" | "centreSize" | "polar";

const PAGE_LABEL: Record<Page, string> = { corners: "By Corners", cornerSize: "By Corner and Size", centreSize: "By Center and Size", polar: "Center and Radius" };

/** The text of every field of the four pages, keyed `page.field`. */
type Fields = Record<string, string>;

const fmt = (um: number, units: LengthUnit): string => String(Number(umTo(um, units).toFixed(units === "mm" ? 4 : 2)));

/** `updateCorners`, `updateCornerSize`, `updateCenterSize`, `updatePolar`: every page's fields from the working copy (`page` left alone when given: the one being typed in). */
function fieldsOf(g: BoardGridItem, units: LengthUnit, keep: Fields = {}, page: Page | null = null): Fields {
  const c = gridDialogCorners(g);
  const all: Record<Page, Fields> = {
    corners: { "corners.sx": fmt(c.start[0], units), "corners.sy": fmt(c.start[1], units), "corners.ex": fmt(c.end[0], units), "corners.ey": fmt(c.end[1], units) },
    cornerSize: { "cornerSize.x": fmt(c.start[0], units), "cornerSize.y": fmt(c.start[1], units), "cornerSize.w": fmt(c.size[0], units), "cornerSize.h": fmt(c.size[1], units) },
    centreSize: { "centreSize.x": fmt(g.x, units), "centreSize.y": fmt(g.y, units), "centreSize.w": fmt(c.size[0], units), "centreSize.h": fmt(c.size[1], units) },
    polar: { "polar.x": fmt(g.x, units), "polar.y": fmt(g.y, units), "polar.r": fmt(g.extent[0], units), "polar.phi": String(g.phi_extent) },
  };
  const out: Fields = {};
  for (const p of Object.keys(all) as Page[]) Object.assign(out, p === page ? Object.fromEntries(Object.keys(all[p]).map((k) => [k, keep[k] ?? all[p][k]!])) : all[p]);
  return out;
}

/** The working copy from the fields of `page` (`onCornersChange`, `onCornerSizeChange`, `onCenterSizeChange`, `onPolarChange`), or null while one of them is not a number. */
function fromPage(g: BoardGridItem, page: Page, f: Fields, units: LengthUnit): BoardGridItem | null {
  const n = (k: string) => Number(f[k]);
  const u = (k: string) => Math.round(umFrom(n(k), units));
  const keys = Object.keys(f).filter((k) => k.startsWith(`${page}.`));
  if (keys.some((k) => !Number.isFinite(n(k)) || f[k]!.trim() === "")) return null;
  switch (page) {
    case "corners":
      return gridFromCorners(g, [u("corners.sx"), u("corners.sy")], [u("corners.ex"), u("corners.ey")]);
    case "cornerSize":
      return gridFromCornerSize(g, [u("cornerSize.x"), u("cornerSize.y")], [u("cornerSize.w"), u("cornerSize.h")]);
    case "centreSize":
      return gridFromCentreSize(g, [u("centreSize.x"), u("centreSize.y")], [u("centreSize.w"), u("centreSize.h")]);
    case "polar":
      // `SetRadiusExtent` keeps the magnitude, `SetPhiExtentDegrees` at least 0.001 degree.
      return { ...g, x: u("polar.x"), y: u("polar.y"), extent: [Math.abs(u("polar.r")), g.extent[1]], phi_extent: Math.max(0.001, Math.abs(n("polar.phi"))) };
  }
}

interface Form {
  work: BoardGridItem;
  fields: Fields;
  page: Page;
  angle: string;
  /** The spacing rows as typed; each type's values are kept in the working copy when the type is switched (`saveSpacingToItem`). */
  spacingX: string;
  spacingY: string;
  tick: string;
  priority: string;
  locked: boolean;
}

/** `updateSpacingRows`: Spacing X / Spacing Y in the user's units, or Radius spacing and Angular spacing (degrees) on a polar grid. */
function spacingOf(g: BoardGridItem, units: LengthUnit): [string, string] {
  return g.kind === "polar" ? [fmt(g.spacing[0], units), String(g.phi_spacing)] : [fmt(g.spacing[0], units), fmt(g.spacing[1], units)];
}

/** `saveSpacingToItem( type )`. */
function savedSpacing(g: BoardGridItem, x: string, y: string, units: LengthUnit): BoardGridItem {
  const sx = Math.max(1, Math.round(umFrom(Number(x) || 0, units)));
  if (g.kind === "polar") return { ...g, spacing: [sx, g.spacing[1]], phi_spacing: Math.max(0.001, Math.abs(Number(y) || 0)) };
  return { ...g, spacing: [sx, Math.max(1, Math.round(umFrom(Number(y) || 0, units)))] };
}

function GridItemDialog() {
  const state = useStudioState();
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const { gridItem: req } = useKicad10Dialogs();
  const units = state.units;
  const item = req ? state.board?.drawings?.grid_items?.find((g) => g.id === req.id) : undefined;
  const [form, setForm] = useState<Form | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!req || !item) {
      setForm(null);
      return;
    }
    const [sx, sy] = spacingOf(item, units);
    setForm({
      work: item,
      fields: fieldsOf(item, units),
      page: item.kind === "polar" ? "polar" : "centreSize",
      angle: String(item.angle),
      spacingX: sx,
      spacingY: sy,
      tick: String(item.tick_interval),
      priority: String(item.priority),
      locked: (state.board?.locked ?? []).includes(item.id),
    });
    setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [req]);

  if (!req || !item || !form) return null;
  const close = () => closeGridItemDialog();
  const polar = form.work.kind === "polar";
  const pages: Page[] = polar ? ["polar"] : ["corners", "cornerSize", "centreSize"];

  /** A field of a page typed: the working copy follows, and the other pages with it. */
  const onField = (page: Page, key: string, text: string) => {
    setForm((f) => {
      if (!f) return f;
      const fields = { ...f.fields, [key]: text };
      const work = fromPage(f.work, page, fields, units);
      return work ? { ...f, work, fields: fieldsOf(work, units, fields, page) } : { ...f, fields };
    });
  };
  /** The orientation typed: the corner fields are world coordinates on the turned outline, so they follow it. */
  const onAngle = (text: string) => {
    setForm((f) => {
      if (!f) return f;
      const deg = Number(text);
      if (!Number.isFinite(deg) || text.trim() === "") return { ...f, angle: text };
      const work = { ...f.work, angle: deg };
      return { ...f, angle: text, work, fields: fieldsOf(work, units) };
    });
  };
  /** `onGridType`: the spacing fields kept under their old meaning, then relabelled for the new type. */
  const onType = (kind: BoardGridItem["kind"]) => {
    setForm((f) => {
      if (!f || f.work.kind === kind) return f;
      const saved = savedSpacing(f.work, f.spacingX, f.spacingY, units);
      const work = { ...saved, kind };
      const [sx, sy] = spacingOf(work, units);
      return { ...f, work, spacingX: sx, spacingY: sy, page: kind === "polar" ? "polar" : "centreSize", fields: fieldsOf(work, units) };
    });
  };

  const ok = async () => {
    const angle = Number(form.angle);
    if (!Number.isFinite(angle)) return setError("The orientation must be a number.");
    let work = savedSpacing({ ...form.work, angle }, form.spacingX, form.spacingY, units);
    // `ToLong`: a tick interval and a priority that are not numbers are 0; below 0 is 0, and a priority of 0 is 1 (`SetAssignedPriority`).
    work = normalizedGridItem({ ...work, tick_interval: Math.max(0, Math.trunc(Number(form.tick) || 0)), priority: Math.max(0, Math.trunc(Number(form.priority) || 0)) });
    const cmds: Cmd[] = [{ op: "pcb_item", verb: "edit_grid_item", id: item.id, grid_item: gridItemCmd(work) }];
    if (form.locked !== (state.board?.locked ?? []).includes(item.id)) cmds.push({ op: "set_locked", ids: [item.id], locked: form.locked });
    if (!(await applyCommands(api, dispatch, cmds, [], null, { keep: true }))) return setError("The board refused the grid.");
    close();
  };

  const num = (page: Page, key: string, label: string) => (
    <>
      <span>{label}</span>
      <input type="number" step="any" value={form.fields[key] ?? ""} onChange={(e) => onField(page, key, e.target.value)} style={{ width: 100 }} data-testid={`grid-${key.replace(".", "-")}`} />
    </>
  );
  const lengthUnit = units;
  const pageBody = (page: Page) => {
    switch (page) {
      case "corners":
        return (
          <>
            {num(page, "corners.sx", `Start Point X (${lengthUnit})`)}
            {num(page, "corners.sy", `Start Point Y (${lengthUnit})`)}
            {num(page, "corners.ex", `End Point X (${lengthUnit})`)}
            {num(page, "corners.ey", `End Point Y (${lengthUnit})`)}
          </>
        );
      case "cornerSize":
        return (
          <>
            {num(page, "cornerSize.x", `Start Point X (${lengthUnit})`)}
            {num(page, "cornerSize.y", `Start Point Y (${lengthUnit})`)}
            {num(page, "cornerSize.w", `Size X (${lengthUnit})`)}
            {num(page, "cornerSize.h", `Size Y (${lengthUnit})`)}
          </>
        );
      case "centreSize":
        return (
          <>
            {num(page, "centreSize.x", `Center X (${lengthUnit})`)}
            {num(page, "centreSize.y", `Center Y (${lengthUnit})`)}
            {num(page, "centreSize.w", `Size X (${lengthUnit})`)}
            {num(page, "centreSize.h", `Size Y (${lengthUnit})`)}
          </>
        );
      case "polar":
        return (
          <>
            {num(page, "polar.x", `Center X (${lengthUnit})`)}
            {num(page, "polar.y", `Center Y (${lengthUnit})`)}
            {num(page, "polar.r", `Radius (${lengthUnit})`)}
            {num(page, "polar.phi", "Angular extent (deg)")}
          </>
        );
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 460 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Grid Properties" data-testid="grid-item-properties">
        <div className="dialog-header">
          <span>Grid Properties</span>
        </div>
        <div className="dialog-body">
          <div style={{ display: "flex", gap: 4, marginBottom: 8 }}>
            {pages.map((p) => (
              <button key={p} className={form.page === p ? "primary" : ""} onClick={() => setForm({ ...form, page: p })} data-testid={`grid-page-${p}`}>
                {PAGE_LABEL[p]}
              </button>
            ))}
          </div>
          <div className="kv-grid" style={{ gridTemplateColumns: "170px 1fr" }}>
            {pageBody(form.page)}
          </div>
          <div className="kv-grid" style={{ gridTemplateColumns: "170px 1fr", marginTop: 10 }}>
            <span>Grid type:</span>
            <select value={form.work.kind} onChange={(e) => onType(e.target.value as BoardGridItem["kind"])} data-testid="grid-type">
              <option value="cartesian">Cartesian</option>
              <option value="polar">Polar</option>
            </select>
            <span>Orientation: (deg)</span>
            <input type="number" step="any" value={form.angle} onChange={(e) => onAngle(e.target.value)} style={{ width: 100 }} data-testid="grid-angle" />
            <span>{polar ? `Radius spacing: (${lengthUnit})` : `Spacing X: (${lengthUnit})`}</span>
            <input type="number" step="any" value={form.spacingX} onChange={(e) => setForm({ ...form, spacingX: e.target.value })} style={{ width: 100 }} data-testid="grid-spacing-x" />
            <span>{polar ? "Angular spacing: (deg)" : `Spacing Y: (${lengthUnit})`}</span>
            <input type="number" step="any" value={form.spacingY} onChange={(e) => setForm({ ...form, spacingY: e.target.value })} style={{ width: 100 }} data-testid="grid-spacing-y" />
            <span>Major tick every: (lines)</span>
            <input type="number" step="1" min={0} value={form.tick} onChange={(e) => setForm({ ...form, tick: e.target.value })} style={{ width: 100 }} data-testid="grid-tick" />
            <span>Priority:</span>
            <input type="number" step="1" min={0} value={form.priority} onChange={(e) => setForm({ ...form, priority: e.target.value })} style={{ width: 100 }} data-testid="grid-priority" />
            <span>Affects:</span>
            <span style={{ display: "flex", gap: 10 }}>
              {(["cursor", "routing", "placement"] as const).map((k) => (
                <label key={k}>
                  <input type="checkbox" checked={form.work.affects[k]} onChange={(e) => setForm({ ...form, work: { ...form.work, affects: { ...form.work.affects, [k]: e.target.checked } } })} data-testid={`grid-affects-${k}`} />
                  {k === "cursor" ? "Cursor" : k === "routing" ? "Routing" : "Placement"}
                </label>
              ))}
            </span>
            <span>Locked</span>
            <label>
              <input type="checkbox" checked={form.locked} onChange={(e) => setForm({ ...form, locked: e.target.checked })} data-testid="grid-locked" />
            </label>
          </div>
          {error && (
            <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }} data-testid="grid-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close} data-testid="grid-cancel">
            Cancel
          </button>
          <button className="primary" onClick={() => void ok()} data-testid="grid-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}


/**
 * Via Stitching Properties (`DIALOG_VIA_STITCH_PROPERTIES`, pcbnew/dialogs/dialog_via_stitch_properties.cpp of KiCad 25ec2335): Stitch Net, Position X / Y (the outline's first corner:
 * a new one moves the outline there), Pattern, Pitch, Seed (the Poisson pattern's only), Via Properties (here the size, drill and layers of the template; KiCad's Configure opens the via
 * dialog on it), Mode and Guard Net (a guard's only), with the sample of the pattern the dialog draws (`buildStitchPreview`). OK is "Edit Via Stitching" (`ShowPropertiesDialog`'s
 * `EditStart`, `Update`, `EditFinish`), or, after Draw Via Stitch Area, "Create Via Stitch": one `set_via_stitch`, the vias placed by the server; Cancel drops a new one.
 */
function ViaStitchDialog() {
  const state = useStudioState();
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const { viaStitch: req } = useKicad10Dialogs();
  const units = state.units;
  const [form, setForm] = useState<{ work: BoardViaStitch; x: string; y: string; pitch: string; seed: string; size: string; drill: string } | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!req) {
      setForm(null);
      return;
    }
    const w = req.stitch;
    const [x, y] = stitchPosition(w);
    const size = w.via?.size_um ?? 0;
    setForm({ work: w, x: fmt(x, units), y: fmt(y, units), pitch: fmt(w.pitch_um || defaultPitchUm(size || 600), units), seed: String(w.seed ?? 0), size: fmt(size, units), drill: fmt(w.via?.drill_um ?? 0, units) });
    setError(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [req]);

  if (!req || !form || !state.board) return null;
  const board = state.board;
  const nets = pcbContext(board, units).nets;
  const copper = board.layers ?? [];
  const close = () => closeViaStitchDialog();
  const set = (patch: Partial<typeof form>) => setForm({ ...form, ...patch });
  const setWork = (patch: Partial<BoardViaStitch>) => setForm({ ...form, work: { ...form.work, ...patch } });
  const mode = modeOf(form.work);
  const layout = layoutOf(form.work);
  const stitching = mode === "stitch";
  const poisson = stitching && layout === "poisson";
  const u = (t: string) => Math.round(umFrom(Number(t), units));
  const preview = stitchPreview(u(form.pitch) || 0, layout, Number(form.seed) >>> 0);

  const ok = async () => {
    const pitch = u(form.pitch);
    const size = u(form.size);
    const drill = u(form.drill);
    // The dialog takes any pitch (0 is `defaultPitch`); the via's fields stand in for `DIALOG_TRACK_VIA_PROPERTIES`, whose `PCB_VIA::ValidateViaParameters` says why a via
    // will not do (`GEOMETRY_MIN_SIZE`, 1 um).
    if (!(size >= 1)) return setError("Via diameter is too small.");
    if (!(drill >= 1)) return setError("Via drill is too small.");
    if (size <= drill) return setError("Via hole size must be smaller than via diameter");
    // `ToULong`: a seed that is not a number leaves the seed as it was.
    const seed = /^\d+$/.test(form.seed.trim()) ? Number(form.seed.trim()) >>> 0 : (form.work.seed ?? 0);
    // "Position: translate the outline to land on the requested first vertex."
    const [x0, y0] = stitchPosition(form.work);
    const [nx, ny] = [u(form.x), u(form.y)];
    let work: BoardViaStitch = { ...form.work, pitch_um: Math.max(0, pitch || 0), seed, via: { from_layer: "F.Cu", to_layer: "B.Cu", ...form.work.via, size_um: size, drill_um: drill } };
    if (Number.isFinite(nx) && Number.isFinite(ny) && (nx !== x0 || ny !== y0)) work = movedStitch(work, nx - x0, ny - y0);
    const before = new Set((board.drawings?.via_stitches ?? []).map((v) => v.id));
    if (!(await api.cmdBatch([viaStitchCmd(req.id, work)]))) return setError("The board refused the via stitching.");
    // A new stitching is selected (`selectItem`), its vias are not.
    if (!req.id) {
      const fresh = await fetchState().catch(() => null);
      const made = (fresh?.drawings?.via_stitches ?? []).find((v) => !before.has(v.id));
      if (made) dispatch({ type: "SET_SELECTION", refs: [made.id] });
    }
    close();
  };

  const scale = preview.extent > 0 ? 180 / (preview.extent + 2 * (u(form.pitch) || 1)) : 1;
  const margin = u(form.pitch) || 1;
  const r = Math.max(1.5, ((u(form.size) || 600) / 2) * scale);
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 560 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Via Stitching Properties" data-testid="via-stitch-properties">
        <div className="dialog-header">
          <span>Via Stitching Properties</span>
        </div>
        <div className="dialog-body" style={{ display: "flex", gap: 12 }}>
          <div className="kv-grid" style={{ gridTemplateColumns: "120px 1fr", flex: 1 }}>
            <span>Stitch Net:</span>
            <select value={form.work.net ?? ""} onChange={(e) => setWork({ net: e.target.value })} data-testid="vst-net">
              <option value="">&lt;no net&gt;</option>
              {nets.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
            <span>Position X: ({units})</span>
            <input type="number" step="any" value={form.x} onChange={(e) => set({ x: e.target.value })} style={{ width: 100 }} data-testid="vst-x" />
            <span>Position Y: ({units})</span>
            <input type="number" step="any" value={form.y} onChange={(e) => set({ y: e.target.value })} style={{ width: 100 }} data-testid="vst-y" />
            <span>Pattern:</span>
            <select value={layout} disabled={!stitching} onChange={(e) => setWork(withLayoutAndMode(form.work, e.target.value as StitchLayout, mode))} data-testid="vst-pattern">
              {PATTERN_CHOICES.map((c) => (
                <option key={c.value} value={c.value}>
                  {c.label}
                </option>
              ))}
            </select>
            <span>Pitch: ({units})</span>
            <input type="number" step="any" value={form.pitch} onChange={(e) => set({ pitch: e.target.value })} style={{ width: 100 }} data-testid="vst-pitch" />
            <span>Seed:</span>
            <input value={form.seed} disabled={!poisson} onChange={(e) => set({ seed: e.target.value })} style={{ width: 100 }} data-testid="vst-seed" />
            <span>Via Properties:</span>
            <span style={{ display: "flex", gap: 4, alignItems: "center", flexWrap: "wrap" }}>
              <input type="number" step="any" value={form.size} onChange={(e) => set({ size: e.target.value })} style={{ width: 70 }} title="Via size" data-testid="vst-size" />
              <span>/</span>
              <input type="number" step="any" value={form.drill} onChange={(e) => set({ drill: e.target.value })} style={{ width: 70 }} title="Via drill" data-testid="vst-drill" /> {units}
              <select value={form.work.via?.from_layer ?? "F.Cu"} onChange={(e) => setWork({ via: { size_um: 0, drill_um: 0, to_layer: "B.Cu", ...form.work.via, from_layer: e.target.value } })} data-testid="vst-from">
                {copper.map((l) => (
                  <option key={l}>{l}</option>
                ))}
              </select>
              <select value={form.work.via?.to_layer ?? "B.Cu"} onChange={(e) => setWork({ via: { size_um: 0, drill_um: 0, from_layer: "F.Cu", ...form.work.via, to_layer: e.target.value } })} data-testid="vst-to">
                {copper.map((l) => (
                  <option key={l}>{l}</option>
                ))}
              </select>
            </span>
            <span>Mode:</span>
            <select value={mode} onChange={(e) => setWork(withLayoutAndMode(form.work, layout, e.target.value as StitchMode))} data-testid="vst-mode">
              {MODE_CHOICES.map((c) => (
                <option key={c.value} value={c.value}>
                  {c.label}
                </option>
              ))}
            </select>
            <span>Guard Net:</span>
            <select value={form.work.guarded_net ?? ""} disabled={stitching} onChange={(e) => setWork({ guarded_net: e.target.value })} data-testid="vst-guard-net">
              <option value="">&lt;no net&gt;</option>
              {nets.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </div>
          {/* `redrawPreview`: the template's vias at the sample's places, framed with a pitch round them; a guard's sample (tracks and their envelope) is not drawn here. */}
          <svg width={180} height={180} style={{ background: "var(--chrome-bg-deep, #000)", flex: "none" }} data-testid="vst-preview">
            {stitching &&
              preview.samples.map(([x, y], i) => <circle key={i} cx={(x + margin) * scale} cy={(y + margin) * scale} r={r} fill={layerColorOf("LAYER_VIA_HOLES")} />)}
          </svg>
        </div>
        {error && (
          <p style={{ color: "var(--chrome-danger)", margin: "8px 12px 0" }} data-testid="vst-error">
            {error}
          </p>
        )}
        <div className="dialog-footer">
          <button onClick={close} data-testid="vst-cancel">
            Cancel
          </button>
          <button className="primary" onClick={() => void ok()} data-testid="vst-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
