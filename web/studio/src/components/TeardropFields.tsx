// The Teardrops part of Track & Via Properties and of Pad Properties' Connections tab (`DIALOG_TRACK_VIA_PROPERTIES_BASE`, `DIALOG_PAD_PROPERTIES_BASE` at KiCad
// 8303b2ad): "Add teardrops on via's (pad's) track connections", "Prefer zone connection" (a pad's), "Allow teardrops to span two track segments", the track width
// limit, best length and width (percent of the anchor's size, `d`), maximum length and width and "Curved edges". A via dialog's boxes are three-state (several vias
// that differ), a pad's two-state. The form is kicad-port/teardropParams.ts's; what an anchor without parameters of its own shows is what Board Setup's block gives
// it. Differs: no "Board contains legacy teardrops." warning -- the studio's derived file is written `(legacy_teardrops yes)` only so that kicad-cli keeps the
// teardrops it is given, which the studio makes from these parameters (Add All Teardrops).
import type { TeardropForm } from "../kicad-port/teardropParams";
import { nmFrom, nmTo, type LengthUnit } from "../state/units";
import { NumberInput } from "./NumberInput";

export function TeardropFields({ form, units, what, onChange }: { form: TeardropForm; units: LengthUnit; what: "via" | "pad"; onChange: (form: TeardropForm) => void }) {
  const set = (patch: Partial<TeardropForm>) => onChange({ ...form, ...patch });
  const check = (label: string, key: "enabled" | "allowTwo" | "preferZone" | "curved", title?: string) => (
    <label style={{ display: "block" }} title={title}>
      <input
        type="checkbox"
        aria-label={label}
        checked={form[key] === true}
        ref={(el) => {
          if (el) el.indeterminate = form[key] == null;
        }}
        onChange={(e) => set({ [key]: e.target.checked })}
      />{" "}
      {label}
    </label>
  );
  const length = (label: string, key: "maxLen" | "maxWidth") => (
    <>
      <span>{label}</span>
      <span>
        <NumberInput label={label} value={form[key]} scale={(v) => nmTo(v, units)} unscale={(v) => Math.round(nmFrom(v, units))} onChange={(v) => set({ [key]: v })} /> {units}
      </span>
    </>
  );
  const percent = (label: string, key: "lenPercent" | "widthPercent" | "hdPercent", suffix: string, title?: string) => (
    <>
      <span title={title}>{label}</span>
      <span>
        <NumberInput label={label} value={form[key]} onChange={(v) => set({ [key]: v })} width={60} /> {suffix}
      </span>
    </>
  );
  return (
    <fieldset style={{ marginTop: 8 }} aria-label="Teardrops">
      <legend>Teardrops</legend>
      {check(what === "via" ? "Add teardrops on via's track connections" : "Add teardrops on pad's track connections", "enabled")}
      {what === "pad" && check("Prefer zone connection", "preferZone", "Do not create teardrops on tracks connected to pads that are also connected to a copper zone.")}
      {check(
        what === "via" ? "Allow teardrops to span two track segments" : "Allow teardrops to span 2 track segments",
        "allowTwo",
        what === "via" ? "Allows a teardrop to extend over the first 2 connected track segments if the first track segment is too short to accommodate the best length." : "Allows a teardrop to spread over 2 tracks if the first track segment is too short",
      )}
      <div className="kv-grid">
        {percent(what === "via" ? "Maximum track width:" : "Track width limit:", "hdPercent", "% ( d )", "Max pad/via size to track width ratio to create a teardrop.\n100 always creates a teardrop.")}
        {percent("Best length (L):", "lenPercent", "% ( d )")}
        {length("Maximum length (L):", "maxLen")}
        {percent("Best width (W):", "widthPercent", "% ( d )")}
        {length("Maximum width (W):", "maxWidth")}
      </div>
      {check("Curved edges", "curved")}
    </fieldset>
  );
}
