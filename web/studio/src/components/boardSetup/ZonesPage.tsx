// Board Setup > Design Rules > Zones -- port of KiCad 25ec2335 pcbnew/dialogs/panel_setup_zones.cpp: the board's default zone settings
// (`BOARD_DESIGN_SETTINGS::GetDefaultZoneSettings`, the project's `defaults.zones.*`) in the zone properties panel (`PANEL_ZONE_PROPERTIES` with no zone and no
// net: `allowNetSpec = false`), and below it the hatch offsets panel (`PANEL_SETUP_ZONE_HATCH_OFFSETS`, `ZoneHatchOffsetsPage`). A new zone starts from these
// (`ZONE_CREATE_HELPER::createNewZone`); 25ec2335's Zone Properties no longer changes them. Apply is `set_zone_defaults` (`DrawingsSection::zone_defaults`),
// which, as KiCad's Board Setup, is no undo step; the derived project carries them as `defaults.zones`.
//
// Differs: the panel's "Locked" box is not here -- the studio keeps a zone's lock in the board's lock list, not in its settings, so a default has none.
import { useEffect, useMemo } from "react";
import { DEFAULT_ZONE_SETTINGS, useStudioState } from "../../state/store";
import type { IslandRemovalMode, PadConnection, ZoneBorderStyle, ZoneDefaults, ZoneSettingsFields, ZoneSmoothing } from "../../api/types";
import { stickySettings, zoneDefaultsCmd } from "../../kicad-port/pcbDrawZone";
import { LengthField, NumberField, PageFrame, usePageApply, usePageDraft, type PageProps, useSetupImport } from "./fields";
import { lengthText } from "../pcbDialogKit";

/** `ZONE_CLEARANCE_MAX_VALUE_MM`, `ZONE_THICKNESS_MIN_VALUE_MM`, `ZONE_BORDER_HATCH_MINDIST_MM` / `MAXDIST_MM` (pcbnew/zones.h), nm. */
const CLEARANCE_MAX = 100_000_000;
const THICKNESS_MIN = 25_000;
const PITCH_MIN = 100_000;
const PITCH_MAX = 2_000_000;
const INT_MAX = 2_147_483_647;

/** The page's settings: the zone settings, and the name (`m_Name`). */
export type ZonesDraft = ZoneSettingsFields & { name: string };

/** `TransferZoneSettingsToWindow` over the board's defaults (`ZONE_SETTINGS()` where it has none). */
export function zonesDraftOf(d: ZoneDefaults | undefined): ZonesDraft {
  return { ...DEFAULT_ZONE_SETTINGS, ...(stickySettings(d) as Partial<ZoneSettingsFields>), name: d?.name ?? "" };
}

/**
 * `PANEL_ZONE_PROPERTIES::AcceptOptions`' checks, in its order, with `UNIT_BINDER::Validate`'s words ("%s must be at least %s." / "%s must be less than %s.", the
 * label without its colon) and the spoke-width message. Null when the page may be applied.
 */
export function zonesProblem(d: ZonesDraft, fmt: (nm: number) => string): string | null {
  const range = (label: string, v: number, lo: number, hi: number) => (v < lo ? `${label} must be at least ${fmt(lo)}.` : v > hi ? `${label} must be less than ${fmt(hi)}.` : null);
  const checks: (string | null)[] = [
    range("Clearance", d.clearance, 0, CLEARANCE_MAX),
    range("Minimum width", d.min_thickness, THICKNESS_MIN, INT_MAX),
    range("Radius", d.corner_radius ?? 0, 0, INT_MAX),
    range("Thermal spoke width", d.thermal_spoke_width, 0, INT_MAX),
    d.fill_mode === "HatchPattern" ? range("Hatch width", d.hatch_thickness, d.min_thickness, INT_MAX) : null,
    d.fill_mode === "HatchPattern" ? range("Hatch gap", d.hatch_gap, d.min_thickness, INT_MAX) : null,
    range("Outline hatch pitch", d.border_hatch_pitch ?? 500_000, PITCH_MIN, PITCH_MAX),
    d.thermal_spoke_width < d.min_thickness ? "Thermal spoke width cannot be smaller than the minimum width." : null,
  ];
  return checks.find((c) => c !== null) ?? null;
}

const PAD_CONNECTIONS: { value: PadConnection; label: string }[] = [
  { value: "Full", label: "Solid" },
  { value: "Thermal", label: "Thermal reliefs" },
  { value: "ThtThermal", label: "Reliefs for PTH" },
  { value: "None", label: "None" },
];
const OUTLINE_DISPLAY: { value: ZoneBorderStyle; label: string }[] = [
  { value: "none", label: "Line" },
  { value: "edge", label: "Hatched" },
  { value: "full", label: "Fully hatched" },
];
const SMOOTHING: { value: ZoneSmoothing; label: string }[] = [
  { value: "none", label: "None" },
  { value: "chamfer", label: "Chamfer" },
  { value: "fillet", label: "Fillet" },
];
const ISLANDS: { value: IslandRemovalMode; label: string }[] = [
  { value: "Always", label: "Always" },
  { value: "Never", label: "Never" },
  { value: "Area", label: "Below area limit" },
];

export function ZonesPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const units = state.units;
  const stored = state.board?.drawings?.zone_defaults;
  const source = useMemo(() => (state.board ? zonesDraftOf(stored) : undefined), [state.board, stored]);
  const page = usePageDraft<ZonesDraft>(source);
  // "Import Settings from Another Board..." (`ImportSettingsFrom`, UI-LEFT-2 BB4)
  useSetupImport("zones", (b) => zonesDraftOf(b.drawings?.zone_defaults ?? undefined), page.importDraft);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const d = page.draft;
  const set = (patch: Partial<ZonesDraft>) => page.edit((x) => ({ ...x, ...patch }));
  const fmt = (nm: number) => `${lengthText(nm, units)} ${units}`;

  const onApply = () => {
    if (!d) return;
    const problem = zonesProblem(d, fmt);
    if (problem) {
      apply.setMessage({ text: problem, bad: true });
      return;
    }
    // `TransferDataFromWindow`: `settings.m_Netcode = NETINFO_LIST::ORPHANED; SetDefaultZoneSettings( settings )`; a corner radius only with smoothing
    const { name, ...settings } = d;
    const form = { ...settings, corner_radius: settings.smoothing === "none" ? 0 : settings.corner_radius, hatch_orientation_mdeg: normalize180(settings.hatch_orientation_mdeg) };
    void apply.run(zoneDefaultsCmd(form, stored?.layers ?? [stored?.layer ?? "F.Cu"], name), page.markClean);
  };

  const row = (label: string, title: string | undefined, control: React.ReactNode) => (
    <>
      <span title={title}>{label}</span>
      <span>{control}</span>
    </>
  );
  const len = (label: string, value: number, onChange: (v: number) => void, title?: string) =>
    row(label, title, <LengthField ariaLabel={label.replace(/:$/, "")} title={title} units={units} value={value} width={80} onChange={(v) => v !== undefined && Number.isFinite(v) && onChange(v)} />);

  return (
    <PageFrame title="Zones" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!d ? null : (
        <>
          <div className="kv-grid" style={{ gridTemplateColumns: "180px 1fr" }}>
            {row("Zone name:", "A unique name for this zone, used to identify it in DRC rules", <input type="text" aria-label="Zone name" value={d.name} onChange={(e) => set({ name: e.target.value })} />)}
          </div>
          <div className="bs-section">Clearances &amp; Pad Connections</div>
          <div className="kv-grid" style={{ gridTemplateColumns: "180px 1fr" }}>
            {len("Clearance:", d.clearance, (v) => set({ clearance: v }), "Copper clearance for this zone (set to 0 to use the netclass clearance)")}
            {len("Minimum width:", d.min_thickness, (v) => set({ min_thickness: v }), "Minimum thickness of filled areas.")}
            {row(
              "Pad connections:",
              "Default pad connection type to zone.\nThis setting can be overridden by local  pad settings",
              <select aria-label="Pad connections" value={d.pad_connection} onChange={(e) => set({ pad_connection: e.target.value as PadConnection })}>
                {PAD_CONNECTIONS.map((o) => (
                  <option key={o.value} value={o.value}>
                    {o.label}
                  </option>
                ))}
              </select>,
            )}
            {len("Thermal relief gap:", d.thermal_gap, (v) => set({ thermal_gap: v }), "The distance that will be kept clear between the filled area of the zone and a pad connected by thermal relief spokes.")}
            {len("Thermal spoke width:", d.thermal_spoke_width, (v) => set({ thermal_spoke_width: v }), "Width of copper in thermal reliefs.")}
          </div>
          <div className="bs-section">Display Overrides</div>
          <div className="kv-grid" style={{ gridTemplateColumns: "180px 1fr" }}>
            {row(
              "Outline display:",
              undefined,
              <select aria-label="Outline display" value={d.border_style ?? "edge"} onChange={(e) => set({ border_style: e.target.value as ZoneBorderStyle })}>
                {OUTLINE_DISPLAY.map((o) => (
                  <option key={o.value} value={o.value}>
                    {o.label}
                  </option>
                ))}
              </select>,
            )}
            {len("Outline hatch pitch:", d.border_hatch_pitch ?? 500_000, (v) => set({ border_hatch_pitch: v }))}
          </div>
          <div className="bs-section">Hatched Fill</div>
          <label className="filter-row" style={{ display: "block" }}>
            <input type="checkbox" aria-label="Hatched fill" checked={d.fill_mode === "HatchPattern"} onChange={(e) => set({ fill_mode: e.target.checked ? "HatchPattern" : "Polygons" })} /> Hatched fill
          </label>
          <div className="kv-grid" style={{ gridTemplateColumns: "180px 1fr" }}>
            {row("Orientation:", undefined, <><NumberField ariaLabel="Orientation" width={80} value={d.hatch_orientation_mdeg / 1000} onChange={(v) => v !== undefined && Number.isFinite(v) && set({ hatch_orientation_mdeg: Math.round(v * 1000) })} /> deg</>)}
            {len("Hatch width:", d.hatch_thickness, (v) => set({ hatch_thickness: v }))}
            {len("Hatch gap:", d.hatch_gap, (v) => set({ hatch_gap: v }))}
            {row(
              "Smoothing effort:",
              "Value of smoothing effort\n0 = no smoothing\n1 = chamfer\n2 = round corners\n3 = round corners (finer shape)",
              <input type="number" aria-label="Smoothing effort" min={0} max={3} step={1} value={d.hatch_smoothing_level} onChange={(e) => set({ hatch_smoothing_level: Math.max(0, Math.min(3, Math.round(Number(e.target.value) || 0))) })} style={{ width: 80 }} />,
            )}
            {row(
              "Smoothing amount:",
              "Ratio between smoothed corners size and the gap between lines\n0 = no smoothing\n1.0 = max radius/chamfer size (half gap value)",
              <input type="number" aria-label="Smoothing amount" min={0} max={1} step={0.1} value={d.hatch_smoothing_value} onChange={(e) => set({ hatch_smoothing_value: Math.max(0, Math.min(1, Number(e.target.value) || 0)) })} style={{ width: 80 }} />,
            )}
          </div>
          <div className="bs-section">Fill</div>
          <div className="kv-grid" style={{ gridTemplateColumns: "180px 1fr" }}>
            {row(
              "Corner smoothing:",
              undefined,
              <select aria-label="Corner smoothing" value={d.smoothing ?? "none"} onChange={(e) => set({ smoothing: e.target.value as ZoneSmoothing })}>
                {SMOOTHING.map((o) => (
                  <option key={o.value} value={o.value}>
                    {o.label}
                  </option>
                ))}
              </select>,
            )}
            {/* `OnCornerSmoothingSelection`: the radius only for a chamfer or a fillet */}
            {d.smoothing && d.smoothing !== "none" && len("Radius:", d.corner_radius ?? 0, (v) => set({ corner_radius: v }))}
            {row(
              "Remove islands:",
              "Choose what to do with unconnected copper islands",
              <select aria-label="Remove islands" value={d.island_removal_mode} onChange={(e) => set({ island_removal_mode: e.target.value as IslandRemovalMode })}>
                {ISLANDS.map((o) => (
                  <option key={o.value} value={o.value}>
                    {o.label}
                  </option>
                ))}
              </select>,
            )}
            {/* `OnRemoveIslandsSelection`: the area limit only "Below area limit"; an area in the units squared */}
            {d.island_removal_mode === "Area" &&
              row(
                "Area limit:",
                "Isolated islands smaller than this will be removed",
                <>
                  <NumberField ariaLabel="Area limit" width={80} value={d.min_island_area / unitNm(units) ** 2} onChange={(v) => v !== undefined && Number.isFinite(v) && set({ min_island_area: Math.round(v * unitNm(units) ** 2) })} /> {units}²
                </>,
              )}
          </div>
        </>
      )}
    </PageFrame>
  );
}

/** nm in one of the display units. */
function unitNm(units: string): number {
  return units === "in" ? 25_400_000 : units === "mil" ? 25_400 : 1_000_000;
}

/** `NormalizeAngle180` in millidegrees: into (-180, 180]. */
function normalize180(mdeg: number): number {
  let a = mdeg % 360_000;
  if (a <= -180_000) a += 360_000;
  if (a > 180_000) a -= 360_000;
  return a;
}
