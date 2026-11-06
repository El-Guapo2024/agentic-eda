// Via Stitching Properties' Configure: `DIALOG_TRACK_VIA_PROPERTIES` in its standalone form on the stitching's template via (`OnViaConfigureClicked`, KiCad 25ec2335) --
// the via rows of Track & Via Properties (kicad-port/viaTemplate.ts says which). OK hands the template back to the stitching dialog, which sends it with its own OK; nothing
// is committed here (`if( !m_standalone ) commit.Push(..)`).
import { useState } from "react";
import { layerDisplayName } from "../kicad-port/boardLayers";
import type { LayerSetup } from "../api/types";
import { ANNULAR_RING_CHOICES, VIA_TYPE_CHOICES, viaLayerDepth } from "../kicad-port/trackViaDialog";
import { viaTemplateForm, viaTemplateFrom, withViaType, type StitchVia, type ViaTemplateForm } from "../kicad-port/viaTemplate";
import { nmFrom, nmTo, type LengthUnit } from "../state/units";

const shown = (nm: number | null, units: LengthUnit) => (nm == null ? "" : String(Number(nmTo(nm, units).toFixed(6))));

export function ViaTemplateDialog({ via, copper, layerSetup, units, onOk, onCancel }: { via: StitchVia; copper: string[]; layerSetup: LayerSetup | undefined | null; units: LengthUnit; onOk: (via: StitchVia) => void; onCancel: () => void }) {
  const [form, setForm] = useState<ViaTemplateForm>(() => viaTemplateForm(via));
  const [texts, setTexts] = useState(() => ({ diameter: shown(via.size_nm, units), drill: shown(via.drill_nm, units) }));
  const [error, setError] = useState<string | null>(null);
  const through = form.type === "through";
  const depth = viaLayerDepth(form.type, form.from, form.to, copper);
  const length = (key: "diameter" | "drill", label: string) => (
    <>
      <span>{label}</span>
      <span>
        <input
          aria-label={label}
          value={texts[key]}
          onChange={(e) => {
            const t = e.target.value;
            setTexts((x) => ({ ...x, [key]: t }));
            const n = Number(t);
            const nm = t.trim() === "" || !Number.isFinite(n) ? null : Math.round(nmFrom(n, units));
            setForm((f) => ({ ...f, [key === "diameter" ? "diameterNm" : "drillNm"]: nm }));
          }}
          style={{ width: 100 }}
          data-testid={`vtp-${key}`}
        />{" "}
        {units}
      </span>
    </>
  );
  const ok = () => {
    const out = viaTemplateFrom(via, form, copper);
    if ("error" in out) return setError(out.error);
    onOk(out.via);
  };
  return (
    <div className="dialog-backdrop" style={{ zIndex: 2100 }} onClick={onCancel}>
      <div className="dialog" style={{ width: 420 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Track & Via Properties" data-testid="via-template-properties">
        <div className="dialog-header">
          <span>Track &amp; Via Properties</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
            <span>Via type:</span>
            <select value={form.type} onChange={(e) => setForm((f) => withViaType(f, e.target.value as ViaTemplateForm["type"], copper))} data-testid="vtp-type">
              {VIA_TYPE_CHOICES.map(([t, label]) => (
                <option key={t} value={t}>
                  {label}
                </option>
              ))}
            </select>
            <span>Start layer:</span>
            <select value={form.from} disabled={through} onChange={(e) => setForm((f) => ({ ...f, from: e.target.value }))} data-testid="vtp-from">
              {copper.map((l) => (
                <option key={l} value={l}>
                  {layerDisplayName(layerSetup, l)}
                </option>
              ))}
            </select>
            <span>End layer:</span>
            <select value={form.to} disabled={through} onChange={(e) => setForm((f) => ({ ...f, to: e.target.value }))} data-testid="vtp-to">
              {copper.map((l) => (
                <option key={l} value={l}>
                  {layerDisplayName(layerSetup, l)}
                </option>
              ))}
            </select>
            {length("diameter", "Diameter:")}
            {length("drill", "Hole:")}
            {depth > 1 && (
              <>
                <span>Annular rings:</span>
                <select value={form.rings} onChange={(e) => setForm((f) => ({ ...f, rings: e.target.value as ViaTemplateForm["rings"] }))} data-testid="vtp-rings">
                  {ANNULAR_RING_CHOICES.map(([r, label]) => (
                    <option key={r} value={r}>
                      {label}
                    </option>
                  ))}
                </select>
              </>
            )}
          </div>
          {error && (
            <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }} data-testid="vtp-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={onCancel} data-testid="vtp-cancel">
            Cancel
          </button>
          <button className="primary" onClick={ok} data-testid="vtp-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
