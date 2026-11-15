// The transmission-line calculators behind Board Setup > Design Rules > Tuning Profiles' track grid (KiCad 25ec2335 common/transline_calculations/): a single
// microstrip, a single stripline, a coupled (differential) microstrip and a coupled stripline, each analysed (geometry -> impedance, effective permittivity,
// unit propagation delay, losses) or synthesized (impedance -> width or gap), as `PANEL_SETUP_TUNING_PROFILE_INFO` drives them. The panel logic is
// kicad-port/tuningCalculator.ts. The Calculator Tools' Transmission Lines panel (kicad-port/calc/translinePanel.ts) uses those four and the coaxial line,
// the coplanar waveguide (with and without a ground plane), the rectangular waveguide and the twisted pair.
//
// Ported from (KiCad 25ec2335):
//   transline_calculation_base.{h,cpp}     the parameter map, the result maps, MinimiseZ0Error1D, SkinDepth, UnitPropagationDelay, EllipticIntegral, coth / sech,
//                                          the Djordjevic-Sarkar overlay (UpdateDielectricModel, GetDispersed*), WanHoorfarQ2, ApplySoldermaskCorrection
//   dielectric_djordjevic_sarkar.{h,cpp}   Fit, ComplexEpsilonAt, EpsilonRealAt, TanDeltaAt
//   microstrip.{h,cpp}                     all of MICROSTRIP
//   stripline.{h,cpp}                      all of STRIPLINE
//   coupled_microstrip.{h,cpp}             all of COUPLED_MICROSTRIP (the DEFAULT synthesis is the Calculator Tools' Synthesize; synth_width stops after 10000 steps)
//   coupled_stripline.{h,cpp}              all of COUPLED_STRIPLINE
//   coax, coplanar, rectwaveguide,
//   twistedpair.{h,cpp}                    all of them, for the Calculator Tools' Transmission Lines panel (kicad-port/calc/translinePanel.ts)
//   units.h                                MU0, E0, C0, ZF0, LOG2DB
//
// The arithmetic is the C++'s, operation for operation and in its order (the same constants, iteration counts and tolerances), so a result agrees with
// KiCad's to the last few bits of a double; tools/transline-golden/ builds KiCad's own sources and transline.test.ts holds this port to its output
// (transline.golden.json). What can differ is the last ulp of a libm function (glibc's against V8's `Math`), which the iterations do not amplify.
//
// Dependency-free, so it runs under `npm run test:unit`.

/** `TRANSLINE_PARAMETERS`: every parameter a calculation takes or gives (only the ones these four calculators use). */
export type TCP =
  | "EPSILONR"
  | "TAND"
  | "H"
  | "H_T"
  | "T"
  | "PHYS_WIDTH"
  | "PHYS_S"
  | "PHYS_LEN"
  | "ROUGH"
  | "MUR"
  | "MURC"
  | "FREQUENCY"
  | "STRIPLINE_A"
  | "Z0"
  | "Z0_E"
  | "Z0_O"
  | "ANG_L"
  | "SIGMA"
  | "SKIN_DEPTH"
  | "LOSS_DIELECTRIC"
  | "LOSS_CONDUCTOR"
  | "EPSILON_EFF"
  | "EPSILON_EFF_EVEN"
  | "EPSILON_EFF_ODD"
  | "UNIT_PROP_DELAY"
  | "UNIT_PROP_DELAY_ODD"
  | "UNIT_PROP_DELAY_EVEN"
  | "ATTEN_COND"
  | "ATTEN_COND_EVEN"
  | "ATTEN_COND_ODD"
  | "ATTEN_DILECTRIC"
  | "ATTEN_DILECTRIC_EVEN"
  | "ATTEN_DILECTRIC_ODD"
  | "Z_DIFF"
  | "Z_COMM"
  | "COUPLING_K"
  | "DIELECTRIC_MODEL_SEL"
  | "EPSILONR_SPEC_FREQ"
  | "SOLDERMASK_PRESENT"
  | "SOLDERMASK_THICKNESS"
  | "SOLDERMASK_EPSILONR"
  | "SOLDERMASK_TAND"
  | "PHYS_DIAM_IN"
  | "PHYS_DIAM_OUT"
  | "CUTOFF_FREQUENCY"
  | "TWISTEDPAIR_TWIST"
  | "TWISTEDPAIR_EPSILONR_ENV"
  | "CPW_BACKMETAL"
  | "SOLDERMASK_FILLS_GAPS";

/** `SYNTHESIZE_OPTS`. */
export type SynthesizeOpts = "DEFAULT" | "FIX_WIDTH" | "FIX_SPACING" | "FROM_ZDIFF_ZCOMM";

/** `TRANSLINE_STATUS` (`TS_ERROR` is ERROR). */
export type TranslineStatus = "OK" | "WARNING" | "TS_ERROR";

/** A result map (`std::unordered_map<TRANSLINE_PARAMETERS, std::pair<double, TRANSLINE_STATUS>>`). */
export type TranslineResults = Map<TCP, [number, TranslineStatus]>;

/** units.h `TRANSLINE_CALCULATIONS`: magnetic constant, permittivity and speed of light in vacuum, wave resistance in vacuum (CODATA 2022), Neper to dB. */
export const MU0 = 12.566370614e-7;
export const E0 = 8.854e-12;
export const C0 = 299792458.0;
export const ZF0 = 376.730313412;
export const LOG2DB = 20.0 / Math.log(10.0);
/** units_scales.h `UNIT_MIL`: mil to metre. */
export const UNIT_MIL = 1e-5 * 2.54;

/** `DIELECTRIC_MODEL`: CONSTANT 0, DJORDJEVIC_SARKAR 1. */
export const DIELECTRIC_MODEL_DJORDJEVIC_SARKAR = 1;

const M_PI = Math.PI;
const M_PI_2 = Math.PI / 2;
const M_E = Math.E;
const { abs: fabs, sqrt, exp, log, log10, pow, tanh, cosh, sinh, acosh, atan, atanh, sin, cos, acos, atan2, floor } = Math;
const isfinite = Number.isFinite;
const clamp = (v: number, lo: number, hi: number) => (v < lo ? lo : hi < v ? hi : v);

// ------------------------------------------------------------------------------------------------------------------ complex helpers (std::complex<double>)

type Complex = [number, number];

/** libgcc `__divdc3` (what g++ emits for `std::complex<double>` division without -ffast-math): Smith's algorithm. */
function cdiv([a, b]: Complex, [c, d]: Complex): Complex {
  if (fabs(c) < fabs(d)) {
    const ratio = c / d;
    const denom = c * ratio + d;
    return [(a * ratio + b) / denom, (b * ratio - a) / denom];
  }
  const ratio = d / c;
  const denom = d * ratio + c;
  return [(b * ratio + a) / denom, (b - a * ratio) / denom];
}

/** `std::log( std::complex<double> )`: `( log( abs( z ) ), arg( z ) )`. */
function clog([re, im]: Complex): Complex {
  return [log(Math.hypot(re, im)), atan2(im, re)];
}

// ------------------------------------------------------------------------------------------------------------------ DIELECTRIC_DJORDJEVIC_SARKAR

/** dielectric_djordjevic_sarkar.cpp: the Kramers-Kronig-consistent wideband dielectric model of Djordjevic et al. 2001, fitted from one (epsR, tan delta, f) point. */
export class DielectricDjordjevicSarkar {
  private m_epsInf = 1.0;
  private m_m = 0.0;
  private m_f1 = 1.0e3;
  private m_f2 = 1.0e12;
  private m_lossless = true;

  /** `Fit`: throws (`std::invalid_argument`) when f1 <= 0, f2 <= f1 or f_spec is outside [f1, f2]. */
  fit(aEpsRSpec: number, aTanDSpec: number, aFSpec: number, aF1 = 1.0e3, aF2 = 1.0e12): void {
    if (aF1 <= 0.0) throw new RangeError("DIELECTRIC_DJORDJEVIC_SARKAR: f1 must be positive");
    if (aF2 <= aF1) throw new RangeError("DIELECTRIC_DJORDJEVIC_SARKAR: f2 must exceed f1");
    if (aFSpec < aF1 || aFSpec > aF2) throw new RangeError("DIELECTRIC_DJORDJEVIC_SARKAR: f_spec outside [f1, f2]");
    this.m_f1 = aF1;
    this.m_f2 = aF2;
    if (aTanDSpec === 0.0) {
      this.m_lossless = true;
      this.m_epsInf = aEpsRSpec;
      this.m_m = 0.0;
      return;
    }
    // ( aF2 + j * aFSpec ) / ( aF1 + j * aFSpec )
    const k = clog(cdiv([aF2, aFSpec], [aF1, aFSpec]));
    this.m_m = (-aTanDSpec * aEpsRSpec) / k[1];
    this.m_epsInf = aEpsRSpec * (1.0 + (aTanDSpec * k[0]) / k[1]);
    this.m_lossless = false;
  }

  /** `ComplexEpsilonAt`. */
  complexEpsilonAt(aF: number): Complex {
    if (this.m_lossless) return [this.m_epsInf, 0.0];
    const l = clog(cdiv([this.m_f2, aF], [this.m_f1, aF]));
    return [this.m_epsInf + this.m_m * l[0], this.m_m * l[1]];
  }

  /** `EpsilonRealAt`. */
  epsilonRealAt(aF: number): number {
    return this.complexEpsilonAt(aF)[0];
  }

  /** `TanDeltaAt`. */
  tanDeltaAt(aF: number): number {
    if (this.m_lossless) return 0.0;
    const eps = this.complexEpsilonAt(aF);
    return -eps[1] / eps[0];
  }
}

// ------------------------------------------------------------------------------------------------------------------ TRANSLINE_CALCULATION_BASE

/** transline_calculation_base.h `TRANSLINE_CALCULATION_BASE`. */
export abstract class TranslineCalculationBase {
  /** `m_parameters`: every parameter of the calculation's list, 0 to start with (`InitProperties`). */
  protected m_parameters = new Map<TCP, number>();
  protected m_analysisStatus: TranslineResults = new Map();
  protected m_synthesisStatus: TranslineResults = new Map();
  /** `m_maxError`: the maximum error for Z0 optimisations. */
  protected static readonly m_maxError = 0.000001;
  protected m_dsModel: DielectricDjordjevicSarkar | null = null;
  /** `m_synthesizeTarget`: which geometry parameter is the unknown during synthesis (set by the UI); `UNKNOWN_ID` is null. */
  protected m_synthesizeTarget: TCP | null = null;

  /** `SetSynthesizeTarget`: the base class takes any; a calculator that solves for given parameters only overrides it to ignore the others. */
  SetSynthesizeTarget(aTarget: TCP): void {
    this.m_synthesizeTarget = aTarget;
  }

  /** `GetSynthesizeTarget`. */
  GetSynthesizeTarget(): TCP | null {
    return this.m_synthesizeTarget;
  }

  constructor(aParameters: readonly TCP[]) {
    for (const p of aParameters) this.m_parameters.set(p, 0.0);
  }

  /** `SetParameter` (`m_parameters.at( aParam ) = aValue`: a parameter not in the list throws). */
  SetParameter(aParam: TCP, aValue: number): void {
    if (!this.m_parameters.has(aParam)) throw new RangeError(`unordered_map::at: ${aParam}`);
    this.m_parameters.set(aParam, aValue);
  }

  /** `GetParameter` (`m_parameters.at( aParam )`). */
  GetParameter(aParam: TCP): number {
    const v = this.m_parameters.get(aParam);
    if (v === undefined) throw new RangeError(`unordered_map::at: ${aParam}`);
    return v;
  }

  /** `GetParameterRef`'s `m_parameters[aParam]`: a parameter not in the map is inserted as 0. */
  protected refParameter(aParam: TCP): number {
    const v = this.m_parameters.get(aParam);
    if (v !== undefined) return v;
    this.m_parameters.set(aParam, 0.0);
    return 0.0;
  }

  protected setRef(aParam: TCP, aValue: number): void {
    this.m_parameters.set(aParam, aValue);
  }

  /** `GetAnalysisResults`. */
  GetAnalysisResults(): TranslineResults {
    this.SetAnalysisResults();
    return this.m_analysisStatus;
  }

  /** `GetSynthesisResults`. */
  GetSynthesisResults(): TranslineResults {
    this.SetSynthesisResults();
    return this.m_synthesisStatus;
  }

  abstract Analyse(): void;
  abstract Synthesize(aOpts: SynthesizeOpts): boolean;
  protected abstract SetAnalysisResults(): void;
  protected abstract SetSynthesisResults(): void;

  protected SetAnalysisResult(aParam: TCP, aValue: number, aStatus: TranslineStatus = "OK"): void {
    this.m_analysisStatus.set(aParam, [aValue, aStatus]);
  }

  protected SetSynthesisResult(aParam: TCP, aValue: number, aStatus: TranslineStatus = "OK"): void {
    this.m_synthesisStatus.set(aParam, [aValue, aStatus]);
  }

  /** `UpdateDielectricModel`: fit the Djordjevic-Sarkar model when DIELECTRIC_MODEL_SEL selects it and the spec frequency is positive, else drop it. */
  UpdateDielectricModel(): void {
    const selected = Math.trunc(this.GetParameter("DIELECTRIC_MODEL_SEL"));
    if (selected !== DIELECTRIC_MODEL_DJORDJEVIC_SARKAR) {
      this.m_dsModel = null;
      return;
    }
    const epsRSpec = this.GetParameter("EPSILONR");
    const tanDSpec = this.GetParameter("TAND");
    const fSpec = this.GetParameter("EPSILONR_SPEC_FREQ");
    if (!isfinite(fSpec) || fSpec <= 0.0) {
      this.m_dsModel = null;
      return;
    }
    try {
      const ds = new DielectricDjordjevicSarkar();
      ds.fit(epsRSpec, tanDSpec, fSpec);
      this.m_dsModel = ds;
    } catch (e) {
      if (!(e instanceof RangeError)) throw e;
      this.m_dsModel = null;
    }
  }

  /** `GetDispersedEpsilonR`. */
  GetDispersedEpsilonR(aF: number): number {
    if (this.m_dsModel) return this.m_dsModel.epsilonRealAt(aF);
    return this.GetParameter("EPSILONR");
  }

  /** `GetDispersedTanDelta`. */
  GetDispersedTanDelta(aF: number): number {
    if (this.m_dsModel) return this.m_dsModel.tanDeltaAt(aF);
    return this.GetParameter("TAND");
  }

  /** `GetSoldermaskDeltaQ`: zero unless the calculator is mask-eligible. */
  GetSoldermaskDeltaQ(_aWOverH: number, _aCOverH: number): number {
    return 0.0;
  }

  /** `WanHoorfarQ2`: the filling factor of the second dielectric layer of a multilayer microstrip (Wan-Hoorfar 2000 eqs. 12 / 13). */
  static WanHoorfarQ2(aU: number, aHBarTop: number): number {
    const wBarEff = aU + (2.0 / M_PI) * log(17.08 * (0.5 * aU + 0.92));
    const denom = wBarEff * M_PI - 4.0;
    if (denom <= 0.0 || !isfinite(denom)) return 0.0;
    const vBar = (2.0 / M_PI) * atan(((2.0 * M_PI) / denom) * (aHBarTop - 1.0));
    const halfPi = 0.5 * M_PI * vBar;
    if (aU >= 1.0) {
      const q1 = 1.0 - log(wBarEff * M_PI - 1.0) / (2.0 * wBarEff);
      const inner = (2.0 * wBarEff * cos(halfPi)) / (2.0 * aHBarTop - 1.0 + vBar) + sin(halfPi);
      if (inner <= 0.0 || !isfinite(inner)) return 0.0;
      const correction = ((1.0 - vBar) / (2.0 * wBarEff)) * log(inner);
      return Math.max(0.0, 1.0 - q1 - correction);
    }
    const logStrip = log(8.0 / aU);
    if (!isfinite(logStrip) || logStrip <= 0.0) return 0.0;
    const q1 = 0.5 + 0.9 / (M_PI * logStrip);
    const bj = (aHBarTop + 1.0) / (aHBarTop + 0.25 * aU - 1.0);
    if (bj <= 0.0 || !isfinite(bj)) return 0.0;
    const acosArg = (sqrt(bj) * (aHBarTop - 1.0 + 0.125 * aU)) / aHBarTop;
    if (!isfinite(acosArg)) return 0.0;
    const correction = (log(bj) * acos(clamp(acosArg, -1.0, 1.0))) / (4.0 * logStrip);
    return Math.max(0.0, 1.0 - q1 - correction);
  }

  /** `ApplySoldermaskCorrection`: the substrate / mask / air correction of an un-coated (eps_eff, tan delta) pair (Bahl-Stuchly 1980 with Wan-Hoorfar 2000). */
  ApplySoldermaskCorrection(aEpsEffUncoated: number, aTanDeltaSubstrate: number, aEpsRSubstrate: number, aWOverH: number, _aF: number): [number, number] {
    if (this.GetParameter("SOLDERMASK_PRESENT") < 0.5) return [aEpsEffUncoated, aTanDeltaSubstrate];
    const C = this.GetParameter("SOLDERMASK_THICKNESS");
    const h = this.GetParameter("H");
    if (C <= 0.0 || h <= 0.0 || !isfinite(C) || !isfinite(h)) return [aEpsEffUncoated, aTanDeltaSubstrate];
    const epsMask = this.GetParameter("SOLDERMASK_EPSILONR");
    const tanDMask = this.GetParameter("SOLDERMASK_TAND");
    if (!isfinite(epsMask) || epsMask <= 1.0) return [aEpsEffUncoated, aTanDeltaSubstrate];
    if (!isfinite(tanDMask) || tanDMask < 0.0) return [aEpsEffUncoated, aTanDeltaSubstrate];
    if (aEpsRSubstrate <= 1.0 || !isfinite(aEpsRSubstrate)) return [aEpsEffUncoated, aTanDeltaSubstrate];
    const deltaQ = this.GetSoldermaskDeltaQ(aWOverH, C / h);
    if (deltaQ <= 0.0) return [aEpsEffUncoated, aTanDeltaSubstrate];
    const epsEffCoated = aEpsEffUncoated + deltaQ * (epsMask - 1.0);
    const qSub = clamp((aEpsEffUncoated - 1.0) / (aEpsRSubstrate - 1.0), 0.0, 1.0);
    const deltaQCapped = Math.min(deltaQ, Math.max(0.0, 1.0 - qSub));
    let tanDCoated = aTanDeltaSubstrate;
    if (epsEffCoated > 0.0) tanDCoated = (qSub * aEpsRSubstrate * aTanDeltaSubstrate + deltaQCapped * epsMask * tanDMask) / epsEffCoated;
    return [epsEffCoated, tanDCoated];
  }

  /**
   * `MinimiseZ0Error1D`: Newton's method on one parameter (`aOptimise`) until the measured impedance (`aMeasure`) is within `m_maxError` of the value it
   * had on entry, at most 251 steps; with `aRecalculateLength` the physical length is recomputed for the angular length. True when the error is small enough.
   */
  protected MinimiseZ0Error1D(aOptimise: TCP, aMeasure: TCP, aRecalculateLength = false): boolean {
    // `GetParameterRef` of the three: inserted as 0 when missing
    this.refParameter(aOptimise);
    const Z0_param = this.refParameter(aMeasure);
    this.refParameter("ANG_L");
    if (!isfinite(Z0_param)) {
      this.setRef(aOptimise, Number.NaN);
      return false;
    }
    const var0 = this.refParameter(aOptimise);
    if (!isfinite(var0) || var0 === 0) this.setRef(aOptimise, 0.001);
    const Z0_dest = this.refParameter(aMeasure);
    const angl_l_dest = this.refParameter("ANG_L");
    let iteration = 0;
    this.Analyse();
    let Z0_current = this.refParameter(aMeasure);
    let error = fabs(Z0_dest - Z0_current);
    while (error > TranslineCalculationBase.m_maxError) {
      iteration++;
      const increment = this.refParameter(aOptimise) / 100.0;
      this.setRef(aOptimise, this.refParameter(aOptimise) + increment);
      this.Analyse();
      const Z0_result = this.refParameter(aMeasure);
      let slope = (Z0_result - Z0_current) / increment;
      slope = (Z0_dest - Z0_current) / slope - increment;
      this.setRef(aOptimise, this.refParameter(aOptimise) + slope);
      if (this.refParameter(aOptimise) <= 0.0) this.setRef(aOptimise, increment);
      this.Analyse();
      Z0_current = this.refParameter(aMeasure);
      error = fabs(Z0_dest - Z0_current);
      if (iteration > 250) break;
    }
    if (aRecalculateLength) {
      for (let k = 0; k < 2; k++) {
        this.setRef(aMeasure, Z0_dest);
        this.setRef("ANG_L", angl_l_dest);
        this.SetParameter("PHYS_LEN", ((C0 / this.GetParameter("FREQUENCY") / sqrt(this.GetParameter("EPSILON_EFF"))) * this.refParameter("ANG_L")) / 2.0 / M_PI);
        if (k === 0) this.Analyse();
      }
    }
    return error <= TranslineCalculationBase.m_maxError;
  }

  /** `SkinDepth`: 1 / sqrt( pi f mu sigma ). */
  protected SkinDepth(): number {
    return 1.0 / sqrt(M_PI * this.GetParameter("FREQUENCY") * this.GetParameter("MURC") * MU0 * this.GetParameter("SIGMA"));
  }

  /** `UnitPropagationDelay`: ps/cm for the effective permittivity. */
  static UnitPropagationDelay(aEpsilonEff: number): number {
    return sqrt(aEpsilonEff) * (1.0e10 / 2.99e8);
  }

  /** `EllipticIntegral`: the complete elliptic integrals of the first and second kind (K, E) by the arithmetic-geometric mean (Abramowitz and Stegun). */
  static EllipticIntegral(arg: number): [number, number] {
    const NR_EPSI = 2.2204460492503131e-16;
    const iMax = 16;
    let k = 0.0;
    let e = 0.0;
    if (arg === 1.0) {
      k = Number.POSITIVE_INFINITY;
      e = 0;
    } else if (arg === Number.NEGATIVE_INFINITY) {
      k = 0;
      e = Number.POSITIVE_INFINITY;
    } else {
      let fk = 1;
      let fe = 1;
      let da = arg;
      if (arg < 0) {
        fk = 1 / sqrt(1 - arg);
        fe = sqrt(1 - arg);
        da = -arg / (1 - arg);
      }
      let a = 1;
      let b = sqrt(1 - da);
      let c = sqrt(da);
      let fr = 0.5;
      let s = fr * c * c;
      let i: number;
      for (i = 0; i < iMax; i++) {
        const t = (a + b) / 2;
        c = (a - b) / 2;
        b = sqrt(a * b);
        a = t;
        fr *= 2;
        s += fr * c * c;
        if (c / a < NR_EPSI) break;
      }
      if (i >= iMax) {
        k = 0;
        e = 0;
      } else {
        k = M_PI_2 / a;
        e = (M_PI_2 * (1 - s)) / a;
        if (arg < 0) {
          k *= fk;
          e *= fe;
        }
      }
    }
    return [k, e];
  }

  /** `coth`. */
  static coth(x: number): number {
    return (exp(2.0 * x) + 1.0) / (exp(2.0 * x) - 1.0);
  }

  /** `sech`. */
  static sech(x: number): number {
    return 2.0 / (exp(x) + exp(-x));
  }
}

const UnitPropagationDelay = TranslineCalculationBase.UnitPropagationDelay;
const WanHoorfarQ2 = TranslineCalculationBase.WanHoorfarQ2;

/** microstrip.h / coupled_microstrip.h `GetSoldermaskDeltaQ`: Wan-Hoorfar q_2 between h_2 = h and h_2 = h + C. */
function microstripSoldermaskDeltaQ(aWOverH: number, aCOverH: number): number {
  if (aWOverH <= 0.0 || aCOverH <= 0.0) return 0.0;
  const q2Coated = WanHoorfarQ2(aWOverH, 1.0 + aCOverH);
  const q2Base = WanHoorfarQ2(aWOverH, 1.0);
  return Math.max(0.0, q2Coated - q2Base);
}

// ------------------------------------------------------------------------------------------------------------------ MICROSTRIP

/** microstrip.{h,cpp} `MICROSTRIP`. */
export class Microstrip extends TranslineCalculationBase {
  Z0_0 = 0.0;
  er_eff_0 = 0.0;
  mur_eff = 0.0;
  w_eff = 0.0;
  Z0_h_1 = 0.0;

  constructor() {
    super(["EPSILONR", "H_T", "H", "PHYS_WIDTH", "T", "Z0", "FREQUENCY", "EPSILON_EFF", "SKIN_DEPTH", "SIGMA", "ROUGH", "TAND", "PHYS_LEN", "MUR", "MURC", "ANG_L", "UNIT_PROP_DELAY", "ATTEN_COND", "ATTEN_DILECTRIC", "DIELECTRIC_MODEL_SEL", "EPSILONR_SPEC_FREQ", "SOLDERMASK_PRESENT", "SOLDERMASK_THICKNESS", "SOLDERMASK_EPSILONR", "SOLDERMASK_TAND"]);
  }

  override GetSoldermaskDeltaQ(aWOverH: number, aCOverH: number): number {
    return microstripSoldermaskDeltaQ(aWOverH, aCOverH);
  }

  /** `MICROSTRIP::Analyse`. */
  Analyse(): void {
    this.UpdateDielectricModel();
    const f = this.GetParameter("FREQUENCY");
    const rawEpsR = this.GetParameter("EPSILONR");
    const rawTanD = this.GetParameter("TAND");
    this.SetParameter("EPSILONR", this.GetDispersedEpsilonR(f));
    this.SetParameter("TAND", this.GetDispersedTanDelta(f));
    this.mur_eff_ms();
    this.microstrip_Z0();
    const erEffUncoated = this.er_eff_0;
    const dispersedEpsR = this.GetParameter("EPSILONR");
    const dispersedTanD = this.GetParameter("TAND");
    const uOverH = this.GetParameter("PHYS_WIDTH") / this.GetParameter("H");
    const [erEffCoated, tanDCoated] = this.ApplySoldermaskCorrection(erEffUncoated, dispersedTanD, dispersedEpsR, uOverH, f);
    if (erEffCoated !== erEffUncoated) {
      this.er_eff_0 = erEffCoated;
      this.Z0_0 *= sqrt(erEffUncoated / erEffCoated);
      this.SetParameter("Z0", this.Z0_0);
    }
    this.dispersion();
    this.line_angle();
    this.SetParameter("TAND", tanDCoated);
    this.attenuation();
    this.SetParameter("EPSILONR", rawEpsR);
    this.SetParameter("TAND", rawTanD);
  }

  /** `MICROSTRIP::Synthesize`: the width for Z0 (Wheeler's estimate, then `MinimiseZ0Error1D`). */
  Synthesize(_aOpts: SynthesizeOpts): boolean {
    const z0_dest = this.GetParameter("Z0");
    const angl_dest = this.GetParameter("ANG_L");
    this.SetParameter("PHYS_WIDTH", this.SynthesizeWidth());
    if (!this.MinimiseZ0Error1D("PHYS_WIDTH", "Z0")) return false;
    this.SetParameter("Z0", z0_dest);
    this.SetParameter("ANG_L", angl_dest);
    const er_eff = this.GetParameter("EPSILON_EFF");
    this.SetParameter("PHYS_LEN", ((C0 / this.GetParameter("FREQUENCY") / sqrt(er_eff * this.mur_eff)) * this.GetParameter("ANG_L")) / 2.0 / M_PI);
    this.Analyse();
    this.SetParameter("Z0", z0_dest);
    this.SetParameter("ANG_L", angl_dest);
    this.SetParameter("PHYS_LEN", ((C0 / this.GetParameter("FREQUENCY") / sqrt(er_eff * this.mur_eff)) * this.GetParameter("ANG_L")) / 2.0 / M_PI);
    return true;
  }

  protected SetAnalysisResults(): void {
    this.SetAnalysisResult("EPSILON_EFF", this.GetParameter("EPSILON_EFF"));
    this.SetAnalysisResult("UNIT_PROP_DELAY", this.GetParameter("UNIT_PROP_DELAY"));
    this.SetAnalysisResult("ATTEN_COND", this.GetParameter("ATTEN_COND"));
    this.SetAnalysisResult("ATTEN_DILECTRIC", this.GetParameter("ATTEN_DILECTRIC"));
    this.SetAnalysisResult("SKIN_DEPTH", this.GetParameter("SKIN_DEPTH"));
    const Z0 = this.GetParameter("Z0");
    const ANG_L = this.GetParameter("ANG_L");
    const L = this.GetParameter("PHYS_LEN");
    const W = this.GetParameter("PHYS_WIDTH");
    this.SetAnalysisResult("Z0", Z0, !isfinite(Z0) || Z0 < 0 ? "TS_ERROR" : "OK");
    this.SetAnalysisResult("ANG_L", ANG_L, !isfinite(ANG_L) || ANG_L < 0 ? "TS_ERROR" : "OK");
    this.SetAnalysisResult("PHYS_LEN", L, !isfinite(L) || L < 0 ? "WARNING" : "OK");
    this.SetAnalysisResult("PHYS_WIDTH", W, !isfinite(W) || W <= 0 ? "WARNING" : "OK");
  }

  protected SetSynthesisResults(): void {
    this.SetSynthesisResult("EPSILON_EFF", this.GetParameter("EPSILON_EFF"));
    this.SetSynthesisResult("UNIT_PROP_DELAY", this.GetParameter("UNIT_PROP_DELAY"));
    this.SetSynthesisResult("ATTEN_COND", this.GetParameter("ATTEN_COND"));
    this.SetSynthesisResult("ATTEN_DILECTRIC", this.GetParameter("ATTEN_DILECTRIC"));
    this.SetSynthesisResult("SKIN_DEPTH", this.GetParameter("SKIN_DEPTH"));
    const Z0 = this.GetParameter("Z0");
    const ANG_L = this.GetParameter("ANG_L");
    const L = this.GetParameter("PHYS_LEN");
    const W = this.GetParameter("PHYS_WIDTH");
    this.SetSynthesisResult("Z0", Z0, !isfinite(Z0) || Z0 < 0 ? "WARNING" : "OK");
    this.SetSynthesisResult("ANG_L", ANG_L, !isfinite(ANG_L) || ANG_L < 0 ? "WARNING" : "OK");
    this.SetSynthesisResult("PHYS_LEN", L, !isfinite(L) || L < 0 ? "TS_ERROR" : "OK");
    this.SetSynthesisResult("PHYS_WIDTH", W, !isfinite(W) || W <= 0 ? "TS_ERROR" : "OK");
  }

  /** `SynthesizeWidth`: Wheeler's closed-form width for Z0. */
  SynthesizeWidth(): number {
    const e_r = this.GetParameter("EPSILONR");
    const a = (this.GetParameter("Z0") / ZF0 / 2 / M_PI) * sqrt((e_r + 1) / 2) + ((e_r - 1) / (e_r + 1)) * (0.23 + 0.11 / e_r);
    const b = ((ZF0 / 2) * M_PI) / (this.GetParameter("Z0") * sqrt(e_r));
    let w_h: number;
    if (a > 1.52) w_h = (8 * exp(a)) / (exp(2 * a) - 2);
    else w_h = (2 / M_PI) * (b - 1 - log(2 * b - 1) + ((e_r - 1) / (2 * e_r)) * (log(b - 1) + 0.39 - 0.61 / e_r));
    return this.GetParameter("H") > 0.0 ? w_h * this.GetParameter("H") : 0;
  }

  /** `Z0_dispersion`: the dispersion correction factor of the characteristic impedance. */
  static Z0_dispersion(u: number, e_r: number, e_r_eff_0: number, e_r_eff_f: number, f_n: number): number {
    const R_1 = 0.03891 * pow(e_r, 1.4);
    const R_2 = 0.267 * pow(u, 7.0);
    const R_3 = 4.766 * exp(-3.228 * pow(u, 0.641));
    const R_4 = 0.016 + pow(0.0514 * e_r, 4.524);
    const R_5 = pow(f_n / 28.843, 12.0);
    const R_6 = 22.2 * pow(u, 1.92);
    const R_7 = 1.206 - 0.3144 * exp(-R_1) * (1.0 - exp(-R_2));
    const R_8 = 1.0 + 1.275 * (1.0 - exp(-0.004625 * R_3 * pow(e_r, 1.674) * pow(f_n / 18.365, 2.745)));
    let tmpf = pow(e_r - 1.0, 6.0);
    const R_9 = 5.086 * R_4 * (R_5 / (0.3838 + 0.386 * R_4)) * (exp(-R_6) / (1.0 + 1.2992 * R_5)) * (tmpf / (1.0 + 10.0 * tmpf));
    const R_10 = 0.00044 * pow(e_r, 2.136) + 0.0184;
    tmpf = pow(f_n / 19.47, 6.0);
    const R_11 = tmpf / (1.0 + 0.0962 * tmpf);
    const R_12 = 1.0 / (1.0 + 0.00245 * u * u);
    const R_13 = 0.9408 * pow(e_r_eff_f, R_8) - 0.9603;
    const R_14 = (0.9408 - R_9) * pow(e_r_eff_0, R_8) - 0.9603;
    const R_15 = 0.707 * R_10 * pow(f_n / 12.3, 1.097);
    const R_16 = 1.0 + 0.0503 * e_r * e_r * R_11 * (1.0 - exp(-pow(u / 15.0, 6.0)));
    const R_17 = R_7 * (1.0 - 1.1241 * (R_12 / R_16) * exp(-0.026 * pow(f_n, 1.15656) - R_15));
    return pow(R_13 / R_14, R_17);
  }

  /** `Z0_homogeneous`: the impedance of a stripline in a homogeneous medium, without cover effects. */
  static Z0_homogeneous(u: number): number {
    const freq = 6.0 + (2.0 * M_PI - 6.0) * exp(-pow(30.666 / u, 0.7528));
    return (ZF0 / (2.0 * M_PI)) * log(freq / u + sqrt(1.0 + 4.0 / (u * u)));
  }

  /** `delta_Z0_cover`: March 1981 Eq. (1) with the corrected P and Q. */
  static delta_Z0_cover(u: number, h2h: number): number {
    const h2hp1 = 1.0 + h2h;
    const P = 270.0 * (1.0 - tanh(0.28 + 1.2 * sqrt(h2h)));
    const qArg = (0.48 * sqrt(Math.max(0.0, u - 1.0))) / (h2hp1 * h2hp1);
    if (qArg >= 1.0) return P;
    const Q = 1.0 - atanh(qArg);
    return P * Q;
  }

  /** `filling_factor`: of a microstrip without cover and of zero thickness. */
  static filling_factor(u: number, e_r: number): number {
    const u2 = u * u;
    const u3 = u2 * u;
    const u4 = u3 * u;
    const a = 1.0 + log((u4 + u2 / 2704) / (u4 + 0.432)) / 49.0 + log(1.0 + u3 / 5929.741) / 18.7;
    const b = 0.564 * pow((e_r - 0.9) / (e_r + 3.0), 0.053);
    return pow(1.0 + 10.0 / u, -a * b);
  }

  /** `delta_q_cover`: March 1981 Eq. (7). */
  static delta_q_cover(h2h: number): number {
    return tanh(1.043 + 0.121 * h2h - 1.164 / h2h);
  }

  /** `delta_q_thickness`. */
  static delta_q_thickness(u: number, t_h: number): number {
    return ((2.0 * log(2.0)) / M_PI) * (t_h / sqrt(u));
  }

  /** `e_r_effective`. */
  static e_r_effective(e_r: number, q: number): number {
    return 0.5 * (e_r + 1.0) + 0.5 * q * (e_r - 1.0);
  }

  /** `delta_u_thickness`: the thickness correction of the normalised width. */
  static delta_u_thickness(u: number, t_h: number, e_r: number): number {
    let delta_u: number;
    if (t_h > 0.0) {
      delta_u = (t_h / M_PI) * log(1.0 + (4.0 * M_E * pow(tanh(sqrt(6.517 * u)), 2.0)) / t_h);
      delta_u = 0.5 * delta_u * (1.0 + 1.0 / cosh(sqrt(e_r - 1.0)));
    } else {
      delta_u = 0.0;
    }
    return delta_u;
  }

  /** `e_r_dispersion`. */
  static e_r_dispersion(u: number, e_r: number, f_n: number): number {
    const P_1 = 0.27488 + u * (0.6315 + 0.525 / pow(1.0 + 0.0157 * f_n, 20.0)) - 0.065683 * exp(-8.7513 * u);
    const P_2 = 0.33622 * (1.0 - exp(-0.03442 * e_r));
    const P_3 = 0.0363 * exp(-4.6 * u) * (1.0 - exp(-pow(f_n / 38.7, 4.97)));
    const P_4 = 1.0 + 2.751 * (1.0 - exp(-pow(e_r / 15.916, 8.0)));
    return P_1 * P_2 * pow((P_3 * P_4 + 0.1844) * f_n, 1.5763);
  }

  conductor_losses(): number {
    let alpha_c: number;
    const e_r_eff_0 = this.er_eff_0;
    const delta = this.GetParameter("SKIN_DEPTH");
    if (this.GetParameter("FREQUENCY") > 0.0) {
      const K = exp(-1.2 * pow(this.Z0_h_1 / ZF0, 0.7));
      let R_s = 1.0 / (this.GetParameter("SIGMA") * delta);
      R_s *= 1.0 + (2.0 / M_PI) * atan(1.4 * pow(this.GetParameter("ROUGH") / delta, 2.0));
      const Q_c = (M_PI * this.Z0_h_1 * this.GetParameter("PHYS_WIDTH") * this.GetParameter("FREQUENCY")) / (R_s * C0 * K);
      alpha_c = (((20.0 * M_PI) / log(10.0)) * this.GetParameter("FREQUENCY") * sqrt(e_r_eff_0)) / (C0 * Q_c);
    } else {
      alpha_c = 0.0;
    }
    return alpha_c;
  }

  dielectric_losses(): number {
    const e_r = this.GetParameter("EPSILONR");
    const e_r_eff_0 = this.er_eff_0;
    return ((20.0 * M_PI) / log(10.0)) * (this.GetParameter("FREQUENCY") / C0) * (e_r / sqrt(e_r_eff_0)) * ((e_r_eff_0 - 1.0) / (e_r - 1.0)) * this.GetParameter("TAND");
  }

  /** `microstrip_Z0`: the static impedance and effective permittivity, corrected for thickness and cover. */
  microstrip_Z0(): void {
    const e_r = this.GetParameter("EPSILONR");
    const h2 = this.GetParameter("H_T");
    const h2h = h2 / this.GetParameter("H");
    let u = this.GetParameter("PHYS_WIDTH") / this.GetParameter("H");
    const t_h = this.GetParameter("T") / this.GetParameter("H");
    const delta_u_1 = Microstrip.delta_u_thickness(u, t_h, 1.0);
    this.Z0_h_1 = Microstrip.Z0_homogeneous(u + delta_u_1);
    const delta_u_r = Microstrip.delta_u_thickness(u, t_h, e_r);
    u += delta_u_r;
    const Z0_h_r = Microstrip.Z0_homogeneous(u);
    const q_inf = Microstrip.filling_factor(u, e_r);
    const q_c = Microstrip.delta_q_cover(h2h);
    const q_t = Microstrip.delta_q_thickness(u, t_h);
    const q = (q_inf - q_t) * q_c;
    const e_r_eff_t = Microstrip.e_r_effective(e_r, q);
    const e_r_eff = e_r_eff_t * pow(this.Z0_h_1 / Z0_h_r, 2.0);
    this.SetParameter("Z0", (Z0_h_r - Microstrip.delta_Z0_cover(u, h2h)) / sqrt(e_r_eff_t));
    this.w_eff = u * this.GetParameter("H");
    this.er_eff_0 = e_r_eff;
    this.Z0_0 = this.GetParameter("Z0");
  }

  /** `dispersion`: the frequency dependence of er_eff and Z0. */
  dispersion(): void {
    const e_r = this.GetParameter("EPSILONR");
    const e_r_eff_0 = this.er_eff_0;
    const u = this.GetParameter("PHYS_WIDTH") / this.GetParameter("H");
    const f_n = (this.GetParameter("FREQUENCY") * this.GetParameter("H")) / 1e6;
    const P = Microstrip.e_r_dispersion(u, e_r, f_n);
    const e_r_eff_f = e_r - (e_r - e_r_eff_0) / (1.0 + P);
    const D = Microstrip.Z0_dispersion(u, e_r, e_r_eff_0, e_r_eff_f, f_n);
    const Z0_f = this.Z0_0 * D;
    this.SetParameter("UNIT_PROP_DELAY", UnitPropagationDelay(e_r_eff_f));
    this.SetParameter("EPSILON_EFF", e_r_eff_f);
    this.SetParameter("Z0", Z0_f);
  }

  attenuation(): void {
    this.SetParameter("SKIN_DEPTH", this.SkinDepth());
    this.SetParameter("ATTEN_COND", this.conductor_losses() * this.GetParameter("PHYS_LEN"));
    this.SetParameter("ATTEN_DILECTRIC", this.dielectric_losses() * this.GetParameter("PHYS_LEN"));
  }

  mur_eff_ms(): void {
    const mur = this.GetParameter("MUR");
    const h = this.GetParameter("H");
    const w = this.GetParameter("PHYS_WIDTH");
    this.mur_eff = (2.0 * mur) / (1.0 + mur + (1.0 - mur) * pow(1.0 + (10.0 * h) / w, -0.5));
  }

  line_angle(): void {
    const e_r_eff = this.GetParameter("EPSILON_EFF");
    const v = C0 / sqrt(e_r_eff * this.mur_eff);
    const lambda_g = v / this.GetParameter("FREQUENCY");
    this.SetParameter("ANG_L", (2.0 * M_PI * this.GetParameter("PHYS_LEN")) / lambda_g);
  }
}

// ------------------------------------------------------------------------------------------------------------------ STRIPLINE

/** stripline.{h,cpp} `STRIPLINE`. */
export class Stripline extends TranslineCalculationBase {
  private unit_prop_delay = 0.0;

  constructor() {
    super(["SKIN_DEPTH", "EPSILON_EFF", "EPSILONR", "T", "STRIPLINE_A", "H", "Z0", "LOSS_CONDUCTOR", "PHYS_LEN", "LOSS_DIELECTRIC", "FREQUENCY", "TAND", "PHYS_WIDTH", "UNIT_PROP_DELAY", "ANG_L", "SIGMA", "MURC", "DIELECTRIC_MODEL_SEL", "EPSILONR_SPEC_FREQ"]);
  }

  /** `STRIPLINE::Analyse`: the impedance as the two half-striplines (strip to the top plane, strip to the bottom plane) in parallel. */
  Analyse(): void {
    this.UpdateDielectricModel();
    const f = this.GetParameter("FREQUENCY");
    const rawEpsR = this.GetParameter("EPSILONR");
    const rawTanD = this.GetParameter("TAND");
    this.SetParameter("EPSILONR", this.GetDispersedEpsilonR(f));
    this.SetParameter("TAND", this.GetDispersedTanDelta(f));
    this.SetParameter("SKIN_DEPTH", this.SkinDepth());
    this.SetParameter("EPSILON_EFF", this.GetParameter("EPSILONR"));
    const t = this.GetParameter("T");
    const a = this.GetParameter("STRIPLINE_A");
    const h = this.GetParameter("H");
    const [z1, ac1] = this.lineImpedance(2.0 * a + t);
    const [z2, ac2] = this.lineImpedance(2.0 * (h - a) - t);
    this.SetParameter("Z0", 2.0 / (1.0 / z1 + 1.0 / z2));
    this.SetParameter("LOSS_CONDUCTOR", this.GetParameter("PHYS_LEN") * (ac1 + ac2));
    this.SetParameter("LOSS_DIELECTRIC", LOG2DB * this.GetParameter("PHYS_LEN") * (M_PI / C0) * this.GetParameter("FREQUENCY") * sqrt(this.GetParameter("EPSILONR")) * this.GetParameter("TAND"));
    this.SetParameter("ANG_L", (2.0 * M_PI * this.GetParameter("PHYS_LEN") * sqrt(this.GetParameter("EPSILONR")) * this.GetParameter("FREQUENCY")) / C0);
    this.unit_prop_delay = UnitPropagationDelay(this.GetParameter("EPSILON_EFF"));
    this.SetParameter("EPSILONR", rawEpsR);
    this.SetParameter("TAND", rawTanD);
  }

  /** `STRIPLINE::Synthesize`: the width for Z0 (`MinimiseZ0Error1D` from the width it has). */
  Synthesize(_aOpts: SynthesizeOpts): boolean {
    return this.MinimiseZ0Error1D("PHYS_WIDTH", "Z0");
  }

  protected SetAnalysisResults(): void {
    this.SetAnalysisResult("EPSILON_EFF", this.GetParameter("EPSILON_EFF"));
    this.SetAnalysisResult("UNIT_PROP_DELAY", this.unit_prop_delay);
    this.SetAnalysisResult("LOSS_CONDUCTOR", this.GetParameter("LOSS_CONDUCTOR"));
    this.SetAnalysisResult("LOSS_DIELECTRIC", this.GetParameter("LOSS_DIELECTRIC"));
    this.SetAnalysisResult("SKIN_DEPTH", this.GetParameter("SKIN_DEPTH"));
    const Z0 = this.GetParameter("Z0");
    const ANG_L = this.GetParameter("ANG_L");
    const L = this.GetParameter("PHYS_LEN");
    const W = this.GetParameter("PHYS_WIDTH");
    const A = this.GetParameter("STRIPLINE_A");
    const T = this.GetParameter("T");
    const H = this.GetParameter("H");
    const ATH_invalid = !isfinite(A) || !isfinite(T) || A + T >= H;
    this.SetAnalysisResult("Z0", Z0, !isfinite(Z0) || Z0 < 0 || ATH_invalid ? "TS_ERROR" : "OK");
    this.SetAnalysisResult("ANG_L", ANG_L, !isfinite(ANG_L) || ANG_L < 0 ? "TS_ERROR" : "OK");
    this.SetAnalysisResult("PHYS_LEN", L, !isfinite(L) || L < 0 ? "WARNING" : "OK");
    this.SetAnalysisResult("PHYS_WIDTH", W, !isfinite(W) || W <= 0 ? "WARNING" : "OK");
    this.SetAnalysisResult("STRIPLINE_A", A, ATH_invalid ? "WARNING" : "OK");
    this.SetAnalysisResult("T", T, ATH_invalid ? "WARNING" : "OK");
    this.SetAnalysisResult("H", H, ATH_invalid ? "WARNING" : "OK");
  }

  protected SetSynthesisResults(): void {
    this.SetSynthesisResult("EPSILON_EFF", this.GetParameter("EPSILON_EFF"));
    this.SetSynthesisResult("UNIT_PROP_DELAY", this.unit_prop_delay);
    this.SetSynthesisResult("LOSS_CONDUCTOR", this.GetParameter("LOSS_CONDUCTOR"));
    this.SetSynthesisResult("LOSS_DIELECTRIC", this.GetParameter("LOSS_DIELECTRIC"));
    this.SetSynthesisResult("SKIN_DEPTH", this.GetParameter("SKIN_DEPTH"));
    const Z0 = this.GetParameter("Z0");
    const ANG_L = this.GetParameter("ANG_L");
    const L = this.GetParameter("PHYS_LEN");
    const W = this.GetParameter("PHYS_WIDTH");
    const invalid = this.GetParameter("STRIPLINE_A") + this.GetParameter("T") >= this.GetParameter("H");
    this.SetSynthesisResult("Z0", Z0, !isfinite(Z0) || Z0 < 0 ? "WARNING" : "OK");
    this.SetSynthesisResult("ANG_L", ANG_L, !isfinite(ANG_L) || ANG_L < 0 ? "WARNING" : "OK");
    this.SetSynthesisResult("PHYS_LEN", L, !isfinite(L) || L < 0 ? "TS_ERROR" : "OK");
    this.SetSynthesisResult("PHYS_WIDTH", W, !isfinite(W) || W <= 0 || invalid ? "TS_ERROR" : "OK");
    this.SetSynthesisResult("STRIPLINE_A", this.GetParameter("STRIPLINE_A"), invalid ? "WARNING" : "OK");
    this.SetSynthesisResult("T", this.GetParameter("T"), invalid ? "WARNING" : "OK");
    this.SetSynthesisResult("H", this.GetParameter("H"), invalid ? "WARNING" : "OK");
  }

  /** `lineImpedance`: the impedance of a strip at `aHeight` from its plane, and its conductor loss (dB/m) -- the C++'s `aAc` out-parameter is the pair's second. */
  private lineImpedance(aHeight: number): [number, number] {
    let ZL: number;
    const T = this.GetParameter("T");
    const W = this.GetParameter("PHYS_WIDTH");
    const ER = this.GetParameter("EPSILONR");
    const hmt = aHeight - T;
    let aAc = sqrt(this.GetParameter("FREQUENCY") / this.GetParameter("SIGMA") / 17.2);
    if (W / hmt >= 0.35) {
      ZL = W + (2.0 * aHeight * log((2.0 * aHeight - T) / hmt) - T * log((aHeight * aHeight) / hmt / hmt - 1.0)) / M_PI;
      ZL = (ZF0 * hmt) / sqrt(ER) / 4.0 / ZL;
      aAc *= (2.02e-6 * ER * ZL) / hmt;
      aAc *= 1.0 + (2.0 * W) / hmt + ((aHeight + T) / hmt / M_PI) * log((2.0 * aHeight) / T - 1.0);
    } else {
      let tdw = T / W;
      if (T / W > 1.0) tdw = W / T;
      let de = 1.0 + (tdw / M_PI) * (1.0 + log((4.0 * M_PI) / tdw)) + 0.236 * pow(tdw, 1.65);
      if (T / W > 1.0) de *= T / 2.0;
      else de *= W / 2.0;
      ZL = (ZF0 / 2.0 / M_PI / sqrt(ER)) * log((4.0 * aHeight) / M_PI / de);
      aAc *= 0.01141 / ZL / de;
      aAc *= de / aHeight + 0.5 + tdw / 2.0 / M_PI + (0.5 / M_PI) * log((4.0 * M_PI) / tdw) + 0.1947 * pow(tdw, 0.65) - 0.0767 * pow(tdw, 1.65);
    }
    return [ZL, aAc];
  }
}

// ------------------------------------------------------------------------------------------------------------------ COUPLED_MICROSTRIP

/** coupled_microstrip.{h,cpp} `COUPLED_MICROSTRIP` (Kirschning-Jansen 1984). */
export class CoupledMicrostrip extends TranslineCalculationBase {
  private m_aux_microstrip = new Microstrip();
  private w_t_e = 0.0;
  private w_t_o = 0.0;
  private er_eff_e_0 = 0.0;
  private er_eff_o_0 = 0.0;
  private Z0_e_0 = 0.0;
  private Z0_o_0 = 0.0;
  private er_eff_e = 0.0;
  private er_eff_o = 0.0;
  private prop_delay_e = 0.0;
  private prop_delay_o = 0.0;
  private atten_cond_e = 0.0;
  private atten_cond_o = 0.0;
  private atten_dielectric_e = 0.0;
  private atten_dielectric_o = 0.0;
  private ang_l_e = 0.0;
  private ang_l_o = 0.0;
  private Zdiff = 0.0;

  constructor() {
    super(["Z0_E", "Z0_O", "Z_DIFF", "ANG_L", "PHYS_WIDTH", "PHYS_LEN", "PHYS_S", "H", "T", "H_T", "FREQUENCY", "EPSILONR", "EPSILON_EFF_EVEN", "EPSILON_EFF_ODD", "UNIT_PROP_DELAY_EVEN", "UNIT_PROP_DELAY_ODD", "ATTEN_COND_EVEN", "ATTEN_COND_ODD", "ATTEN_DILECTRIC_EVEN", "ATTEN_DILECTRIC_ODD", "SKIN_DEPTH", "MURC", "SIGMA", "ROUGH", "TAND", "DIELECTRIC_MODEL_SEL", "EPSILONR_SPEC_FREQ", "SOLDERMASK_PRESENT", "SOLDERMASK_THICKNESS", "SOLDERMASK_EPSILONR", "SOLDERMASK_TAND"]);
  }

  override GetSoldermaskDeltaQ(aWOverH: number, aCOverH: number): number {
    return microstripSoldermaskDeltaQ(aWOverH, aCOverH);
  }

  /** `COUPLED_MICROSTRIP::Analyse`. */
  Analyse(): void {
    this.UpdateDielectricModel();
    const f = this.GetParameter("FREQUENCY");
    const rawEpsR = this.GetParameter("EPSILONR");
    const rawTanD = this.GetParameter("TAND");
    this.SetParameter("EPSILONR", this.GetDispersedEpsilonR(f));
    this.SetParameter("TAND", this.GetDispersedTanDelta(f));
    this.delta_u_thickness();
    this.er_eff_static();
    this.Z0_even_odd();
    const dispersedEpsR = this.GetParameter("EPSILONR");
    const dispersedTanD = this.GetParameter("TAND");
    const uOverH = this.GetParameter("PHYS_WIDTH") / this.GetParameter("H");
    const [erEvenCoated, tanDEvenCoated] = this.ApplySoldermaskCorrection(this.er_eff_e_0, dispersedTanD, dispersedEpsR, uOverH, f);
    const [erOddCoated, tanDOddCoated] = this.ApplySoldermaskCorrection(this.er_eff_o_0, dispersedTanD, dispersedEpsR, uOverH, f);
    if (erEvenCoated !== this.er_eff_e_0) {
      this.Z0_e_0 *= sqrt(this.er_eff_e_0 / erEvenCoated);
      this.er_eff_e_0 = erEvenCoated;
    }
    if (erOddCoated !== this.er_eff_o_0) {
      this.Z0_o_0 *= sqrt(this.er_eff_o_0 / erOddCoated);
      this.er_eff_o_0 = erOddCoated;
    }
    this.er_eff_freq();
    this.Z0_dispersion();
    const tanDBlended = 0.5 * (tanDEvenCoated + tanDOddCoated);
    this.SetParameter("TAND", tanDBlended);
    this.attenuation();
    this.line_angle();
    this.diff_impedance();
    this.SetParameter("EPSILONR", rawEpsR);
    this.SetParameter("TAND", rawTanD);
  }

  /**
   * `COUPLED_MICROSTRIP::Synthesize`: FIX_WIDTH solves the gap, FIX_SPACING the width, each on the odd-mode impedance; any other option (DEFAULT, and
   * FROM_ZDIFF_ZCOMM, which this calculator does not read) solves width and gap together for Z0_E and Z0_O: `synth_width`'s closed-form first guess,
   * then Newton-Raphson on `syn_fun` (at most 250 steps), the physical length from the angular length, and a last `Analyse`.
   */
  Synthesize(aOpts: SynthesizeOpts): boolean {
    if (aOpts === "FIX_WIDTH") return this.MinimiseZ0Error1D("PHYS_S", "Z0_O", false);
    if (aOpts === "FIX_SPACING") return this.MinimiseZ0Error1D("PHYS_WIDTH", "Z0_O", false);
    const eps = 1e-4;
    const Z0_e = this.GetParameter("Z0_E");
    const Z0_o = this.GetParameter("Z0_O");
    this.ang_l_e = this.GetParameter("ANG_L");
    this.ang_l_o = this.GetParameter("ANG_L");
    const ang_l_dest = this.GetParameter("ANG_L");
    this.synth_width();
    let w_h = this.GetParameter("PHYS_WIDTH") / this.GetParameter("H");
    let s_h = this.GetParameter("PHYS_S") / this.GetParameter("H");
    let f1 = 0;
    let f2 = 0;
    let err: number;
    let iters = 0;
    do {
      ++iters;
      let [ft1, ft2] = this.syn_fun(s_h + eps, w_h, Z0_e, Z0_o);
      const j11 = (ft1 - f1) / eps;
      const j21 = (ft2 - f2) / eps;
      [ft1, ft2] = this.syn_fun(s_h, w_h + eps, Z0_e, Z0_o);
      const j12 = (ft1 - f1) / eps;
      const j22 = (ft2 - f2) / eps;
      const d_s_h = (-f1 * j22 + f2 * j12) / (j11 * j22 - j21 * j12);
      const d_w_h = (-f2 * j11 + f1 * j21) / (j11 * j22 - j21 * j12);
      s_h += d_s_h;
      w_h += d_w_h;
      [f1, f2] = this.syn_fun(s_h, w_h, Z0_e, Z0_o);
      err = sqrt(f1 * f1 + f2 * f2);
    } while (err > 1e-4 && iters < 250);
    if (err > 1e-4) return false;
    this.SetParameter("PHYS_S", s_h * this.GetParameter("H"));
    this.SetParameter("PHYS_WIDTH", w_h * this.GetParameter("H"));
    const le = ((C0 / this.GetParameter("FREQUENCY") / sqrt(this.er_eff_e)) * ang_l_dest) / 2.0 / M_PI;
    const lo = ((C0 / this.GetParameter("FREQUENCY") / sqrt(this.er_eff_o)) * ang_l_dest) / 2.0 / M_PI;
    this.SetParameter("PHYS_LEN", sqrt(le * lo));
    this.Analyse();
    this.SetParameter("ANG_L", ang_l_dest);
    this.SetParameter("Z0_E", Z0_e);
    this.SetParameter("Z0_O", Z0_o);
    return true;
  }

  /** `syn_err_fun`: the closed-form even / odd width errors of a (gap, width) guess against the single-line widths (the C++'s out-parameters are the pair). */
  private syn_err_fun(s_h: number, w_h: number, e_r: number, w_h_se: number, w_h_so: number): [number, number] {
    const g = cosh(0.5 * M_PI * s_h);
    const he = cosh(M_PI * w_h + 0.5 * M_PI * s_h);
    let f1 = (2.0 / M_PI) * acosh((2.0 * he - g + 1.0) / (g + 1.0));
    let f2 = (2.0 / M_PI) * acosh((2.0 * he - g - 1.0) / (g - 1.0));
    if (e_r <= 6.0) f2 += (4.0 / (M_PI * (1.0 + e_r / 2.0))) * acosh(1.0 + (2.0 * w_h) / s_h);
    else f2 += (1.0 / M_PI) * acosh(1.0 + (2.0 * w_h) / s_h);
    f1 -= w_h_se;
    f2 -= w_h_so;
    return [f1, f2];
  }

  /**
   * `synth_width`: Wheeler's single-line widths for Z0_E / 2 and Z0_O / 2, Akhtarzad's guess of gap and width from them, refined by Newton-Raphson on
   * `syn_err_fun`. KiCad loops until the error is below 1e-4 with no limit; this stops after 10000 steps so a guess that never converges cannot hang the
   * page (the outer Newton-Raphson then starts from where it stopped, as KiCad's would if it ever returned).
   */
  private synth_width(): void {
    const eps = 1e-4;
    let f1 = 0;
    let f2 = 0;
    const e_r = this.GetParameter("EPSILONR");
    let Z0 = this.GetParameter("Z0_E") / 2.0;
    let a = exp((Z0 * sqrt(e_r + 1.0)) / 42.4) - 1.0;
    const w_h_se = (8.0 * sqrt(a * ((7.0 + 4.0 / e_r) / 11.0) + (1.0 + 1.0 / e_r) / 0.81)) / a;
    Z0 = this.GetParameter("Z0_O") / 2.0;
    a = exp((Z0 * sqrt(e_r + 1.0)) / 42.4) - 1.0;
    const w_h_so = (8.0 * sqrt(a * ((7.0 + 4.0 / e_r) / 11.0) + (1.0 + 1.0 / e_r) / 0.81)) / a;
    const ce = cosh(0.5 * M_PI * w_h_se);
    const co = cosh(0.5 * M_PI * w_h_so);
    let s_h = (2.0 / M_PI) * acosh((ce + co - 2.0) / (co - ce));
    let w_h = acosh((ce * co - 1.0) / (co - ce)) / M_PI - s_h / 2.0;
    this.SetParameter("PHYS_S", s_h * this.GetParameter("H"));
    this.SetParameter("PHYS_WIDTH", w_h * this.GetParameter("H"));
    [f1, f2] = this.syn_err_fun(s_h, w_h, e_r, w_h_se, w_h_so);
    let err: number;
    let guard = 0;
    do {
      let [ft1, ft2] = this.syn_err_fun(s_h + eps, w_h, e_r, w_h_se, w_h_so);
      const j11 = (ft1 - f1) / eps;
      const j21 = (ft2 - f2) / eps;
      [ft1, ft2] = this.syn_err_fun(s_h, w_h + eps, e_r, w_h_se, w_h_so);
      const j12 = (ft1 - f1) / eps;
      const j22 = (ft2 - f2) / eps;
      const d_s_h = (-f1 * j22 + f2 * j12) / (j11 * j22 - j21 * j12);
      const d_w_h = (-f2 * j11 + f1 * j21) / (j11 * j22 - j21 * j12);
      s_h += d_s_h;
      w_h += d_w_h;
      [f1, f2] = this.syn_err_fun(s_h, w_h, e_r, w_h_se, w_h_so);
      err = sqrt(f1 * f1 + f2 * f2);
    } while (err > 1e-4 && ++guard < 10000);
    this.SetParameter("PHYS_S", s_h * this.GetParameter("H"));
    this.SetParameter("PHYS_WIDTH", w_h * this.GetParameter("H"));
  }

  /** `syn_fun`: analyse at a (gap, width) guess; the even / odd impedance errors. */
  private syn_fun(s_h: number, w_h: number, Z0_e: number, Z0_o: number): [number, number] {
    this.SetParameter("PHYS_S", s_h * this.GetParameter("H"));
    this.SetParameter("PHYS_WIDTH", w_h * this.GetParameter("H"));
    this.Analyse();
    return [this.GetParameter("Z0_E") - Z0_e, this.GetParameter("Z0_O") - Z0_o];
  }

  private publish(set: (p: TCP, v: number, s?: TranslineStatus) => void, impedanceFailure: TranslineStatus, geometryFailure: TranslineStatus): void {
    set("EPSILON_EFF_EVEN", this.er_eff_e);
    set("EPSILON_EFF_ODD", this.er_eff_o);
    set("UNIT_PROP_DELAY_EVEN", this.prop_delay_e);
    set("UNIT_PROP_DELAY_ODD", this.prop_delay_o);
    set("ATTEN_COND_EVEN", this.atten_cond_e);
    set("ATTEN_COND_ODD", this.atten_cond_o);
    set("ATTEN_DILECTRIC_EVEN", this.atten_dielectric_e);
    set("ATTEN_DILECTRIC_ODD", this.atten_dielectric_o);
    set("SKIN_DEPTH", this.GetParameter("SKIN_DEPTH"));
    set("Z_DIFF", this.Zdiff);
    const Z0_E = this.GetParameter("Z0_E");
    const Z0_O = this.GetParameter("Z0_O");
    const ANG_L = sqrt(this.ang_l_e * this.ang_l_o);
    const W = this.GetParameter("PHYS_WIDTH");
    const L = this.GetParameter("PHYS_LEN");
    const S = this.GetParameter("PHYS_S");
    set("Z0_E", Z0_E, !isfinite(Z0_E) || Z0_E <= 0 ? impedanceFailure : "OK");
    set("Z0_O", Z0_O, !isfinite(Z0_O) || Z0_O <= 0 ? impedanceFailure : "OK");
    set("ANG_L", ANG_L, !isfinite(ANG_L) || ANG_L < 0 ? impedanceFailure : "OK");
    set("PHYS_WIDTH", W, !isfinite(W) || W <= 0 ? geometryFailure : "OK");
    set("PHYS_LEN", L, !isfinite(L) || L < 0 ? geometryFailure : "OK");
    set("PHYS_S", S, !isfinite(S) || S <= 0 ? geometryFailure : "OK");
  }

  /** `SetAnalysisResults`: a bad impedance is an error, bad geometry a warning. */
  protected SetAnalysisResults(): void {
    this.publish((p, v, s) => this.SetAnalysisResult(p, v, s), "TS_ERROR", "WARNING");
  }

  /** `SetSynthesisResults`: the other way round. */
  protected SetSynthesisResults(): void {
    this.publish((p, v, s) => this.SetSynthesisResult(p, v, s), "WARNING", "TS_ERROR");
  }

  /** `delta_u_thickness_single`: Atwater 1989. */
  private delta_u_thickness_single(u: number, t_h: number): number {
    if (t_h > 0.0) return ((1.25 * t_h) / M_PI) * (1.0 + log((2.0 + (4.0 * M_PI * u - 2.0) / (1.0 + exp(-100.0 * (u - 1.0 / (2.0 * M_PI))))) / t_h));
    return 0.0;
  }

  /** `delta_u_thickness`: Jansen 1978, the even- and odd-mode widths corrected for the strip thickness. */
  private delta_u_thickness(): void {
    const e_r = this.GetParameter("EPSILONR");
    const u = this.GetParameter("PHYS_WIDTH") / this.GetParameter("H");
    const g = this.GetParameter("PHYS_S") / this.GetParameter("H");
    const t_h = this.GetParameter("T") / this.GetParameter("H");
    let delta_u_e: number;
    let delta_u_o: number;
    if (t_h > 0.0) {
      const delta_u = this.delta_u_thickness_single(u, t_h);
      const delta_t = t_h / (g * e_r);
      delta_u_e = delta_u * (1.0 - 0.5 * exp((-0.69 * delta_u) / delta_t));
      delta_u_o = delta_u_e + delta_t;
    } else {
      delta_u_e = delta_u_o = 0.0;
    }
    this.w_t_e = this.GetParameter("PHYS_WIDTH") + delta_u_e * this.GetParameter("H");
    this.w_t_o = this.GetParameter("PHYS_WIDTH") + delta_u_o * this.GetParameter("H");
  }

  /** `compute_single_line`: the zero-thickness single microstrip of the same width. */
  private compute_single_line(): void {
    const m = this.m_aux_microstrip;
    m.SetParameter("EPSILONR", this.GetParameter("EPSILONR"));
    m.SetParameter("PHYS_WIDTH", this.GetParameter("PHYS_WIDTH"));
    m.SetParameter("H", this.GetParameter("H"));
    m.SetParameter("T", 0.0);
    m.SetParameter("H_T", 1e12);
    m.SetParameter("FREQUENCY", this.GetParameter("FREQUENCY"));
    m.SetParameter("MURC", this.GetParameter("MURC"));
    m.microstrip_Z0();
    m.dispersion();
  }

  private filling_factor_even(u: number, g: number, e_r: number): number {
    const v = (u * (20.0 + g * g)) / (10.0 + g * g) + g * exp(-g);
    const v3 = v * v * v;
    const v4 = v3 * v;
    const a_e = 1.0 + log((v4 + (v * v) / 2704.0) / (v4 + 0.432)) / 49.0 + log(1.0 + v3 / 5929.741) / 18.7;
    const b_e = 0.564 * pow((e_r - 0.9) / (e_r + 3.0), 0.053);
    return pow(1.0 + 10.0 / v, -a_e * b_e);
  }

  private filling_factor_odd(u: number, g: number, e_r: number): number {
    const b_odd = (0.747 * e_r) / (0.15 + e_r);
    const c_odd = b_odd - (b_odd - 0.207) * exp(-0.414 * u);
    const d_odd = 0.593 + 0.694 * exp(-0.562 * u);
    return exp(-c_odd * pow(g, d_odd));
  }

  private delta_q_cover_even(h2h: number): number {
    return h2h <= 39 ? tanh(1.626 + 0.107 * h2h - 1.733 / sqrt(h2h)) : 1.0;
  }

  private delta_q_cover_odd(h2h: number): number {
    return h2h <= 7 ? tanh(9.575 / (7.0 - h2h) - 2.965 + 1.68 * h2h - 0.311 * h2h * h2h) : 1.0;
  }

  /** `er_eff_static`: the static even- and odd-mode effective permittivities. */
  private er_eff_static(): void {
    const er = this.GetParameter("EPSILONR");
    this.compute_single_line();
    const er_eff_single = this.m_aux_microstrip.er_eff_0;
    const h2 = this.GetParameter("H_T");
    const u_t_e = this.w_t_e / this.GetParameter("H");
    const u_t_o = this.w_t_o / this.GetParameter("H");
    const g = this.GetParameter("PHYS_S") / this.GetParameter("H");
    const h2h = h2 / this.GetParameter("H");
    const t_h = this.GetParameter("T") / this.GetParameter("H");
    let q_inf = this.filling_factor_even(u_t_e, g, er);
    let q_c = this.delta_q_cover_even(h2h);
    let q_t = Microstrip.delta_q_thickness(u_t_e, t_h);
    let q = (q_inf - q_t) * q_c;
    this.er_eff_e_0 = 0.5 * (er + 1.0) + 0.5 * (er - 1.0) * q;
    q_inf = this.filling_factor_odd(u_t_o, g, er);
    q_c = this.delta_q_cover_odd(h2h);
    q_t = Microstrip.delta_q_thickness(u_t_o, t_h);
    q = (q_inf - q_t) * q_c;
    const a_o = 0.7287 * (er_eff_single - 0.5 * (er + 1.0)) * (1.0 - exp(-0.179 * u_t_o));
    this.er_eff_o_0 = (0.5 * (er + 1.0) + a_o - er_eff_single) * q + er_eff_single;
  }

  private delta_Z0_even_cover(g: number, u: number, h2h: number): number {
    const A = -4.351 / pow(1.0 + h2h, 1.842);
    const B = 6.639 / pow(1.0 + h2h, 1.861);
    const C = -2.291 / pow(1.0 + h2h, 1.9);
    const f_e = 1.0 - atanh(A + (B + C * u) * u);
    const x = pow(10.0, 0.103 * g - 0.159);
    const y = pow(10.0, 0.0492 * g - 0.073);
    const D = 0.747 / sin(0.5 * M_PI * x);
    const E = 0.725 * sin(0.5 * M_PI * y);
    const F = pow(10.0, 0.11 - 0.0947 * g);
    const g_e = 270.0 * (1.0 - tanh(D + E * sqrt(1.0 + h2h) - F / (1.0 + h2h)));
    return f_e * g_e;
  }

  private delta_Z0_odd_cover(g: number, u: number, h2h: number): number {
    const J = tanh(pow(1.0 + h2h, 1.585) / 6.0);
    const f_o = pow(u, J);
    const G = 2.178 - 0.796 * g;
    const K = g > 0.858 ? log10(20.492 * pow(g, 0.174)) : 1.3;
    const L = g > 0.873 ? 2.51 * pow(g, -0.462) : 2.674;
    const g_o = 270.0 * (1.0 - tanh(G + K * sqrt(1.0 + h2h) - L / (1.0 + h2h)));
    return f_o * g_o;
  }

  /** `Z0_even_odd`: the static even- and odd-mode impedances. */
  private Z0_even_odd(): void {
    const h2 = this.GetParameter("H_T");
    const u_t_e = this.w_t_e / this.GetParameter("H");
    const u_t_o = this.w_t_o / this.GetParameter("H");
    const g = this.GetParameter("PHYS_S") / this.GetParameter("H");
    const h2h = h2 / this.GetParameter("H");
    const Z0_single = this.m_aux_microstrip.Z0_0;
    const er_eff_single = this.m_aux_microstrip.er_eff_0;
    let er_eff = this.er_eff_e_0;
    const Q_1 = 0.8695 * pow(u_t_e, 0.194);
    const Q_2 = 1.0 + 0.7519 * g + 0.189 * pow(g, 2.31);
    const Q_3 = 0.1975 + pow(16.6 + pow(8.4 / g, 6.0), -0.387) + log(pow(g, 10.0) / (1.0 + pow(g / 3.4, 10.0))) / 241.0;
    const Q_4 = (2.0 * Q_1) / (Q_2 * (exp(-g) * pow(u_t_e, Q_3) + (2.0 - exp(-g)) * pow(u_t_e, -Q_3)));
    this.Z0_e_0 = (Z0_single * sqrt(er_eff_single / er_eff)) / (1.0 - (sqrt(er_eff_single) * Q_4 * Z0_single) / ZF0);
    const delta_Z0_e_0 = this.delta_Z0_even_cover(g, u_t_e, h2h) / sqrt(er_eff);
    this.Z0_e_0 = this.Z0_e_0 - delta_Z0_e_0;
    er_eff = this.er_eff_o_0;
    const Q_5 = 1.794 + 1.14 * log(1.0 + 0.638 / (g + 0.517 * pow(g, 2.43)));
    const Q_6 = 0.2305 + log(pow(g, 10.0) / (1.0 + pow(g / 5.8, 10.0))) / 281.3 + log(1.0 + 0.598 * pow(g, 1.154)) / 5.1;
    const Q_7 = (10.0 + 190.0 * g * g) / (1.0 + 82.3 * g * g * g);
    const Q_8 = exp(-6.5 - 0.95 * log(g) - pow(g / 0.15, 5.0));
    const Q_9 = log(Q_7) * (Q_8 + 1.0 / 16.5);
    const Q_10 = (Q_2 * Q_4 - Q_5 * exp(log(u_t_o) * Q_6 * pow(u_t_o, -Q_9))) / Q_2;
    this.Z0_o_0 = (Z0_single * sqrt(er_eff_single / er_eff)) / (1.0 - (sqrt(er_eff_single) * Q_10 * Z0_single) / ZF0);
    const delta_Z0_o_0 = this.delta_Z0_odd_cover(g, u_t_o, h2h) / sqrt(er_eff);
    this.Z0_o_0 = this.Z0_o_0 - delta_Z0_o_0;
  }

  /** `er_eff_freq`: the even- and odd-mode effective permittivities with frequency, and their unit delays. */
  private er_eff_freq(): void {
    const ER = this.GetParameter("EPSILONR");
    const u = this.GetParameter("PHYS_WIDTH") / this.GetParameter("H");
    const g = this.GetParameter("PHYS_S") / this.GetParameter("H");
    const f_n = (this.GetParameter("FREQUENCY") * this.GetParameter("H")) / 1e6;
    let er_eff = this.er_eff_e_0;
    const P_1 = 0.27488 + (0.6315 + 0.525 / pow(1.0 + 0.0157 * f_n, 20.0)) * u - 0.065683 * exp(-8.7513 * u);
    const P_2 = 0.33622 * (1.0 - exp(-0.03442 * ER));
    const P_3 = 0.0363 * exp(-4.6 * u) * (1.0 - exp(-pow(f_n / 38.7, 4.97)));
    const P_4 = 1.0 + 2.751 * (1.0 - exp(-pow(ER / 15.916, 8.0)));
    const P_5 = 0.334 * exp(-3.3 * pow(ER / 15.0, 3.0)) + 0.746;
    const P_6 = P_5 * exp(-pow(f_n / 18.0, 0.368));
    const P_7 = 1.0 + 4.069 * P_6 * pow(g, 0.479) * exp(-1.347 * pow(g, 0.595) - 0.17 * pow(g, 2.5));
    const F_e = P_1 * P_2 * pow((P_3 * P_4 + 0.1844 * P_7) * f_n, 1.5763);
    this.er_eff_e = ER - (ER - er_eff) / (1.0 + F_e);
    this.prop_delay_e = UnitPropagationDelay(this.er_eff_e);
    er_eff = this.er_eff_o_0;
    const P_8 = 0.7168 * (1.0 + 1.076 / (1.0 + 0.0576 * (ER - 1.0)));
    const P_9 = P_8 - 0.7913 * (1.0 - exp(-pow(f_n / 20.0, 1.424))) * atan(2.481 * pow(ER / 8.0, 0.946));
    const P_10 = 0.242 * pow(ER - 1.0, 0.55);
    const P_11 = 0.6366 * (exp(-0.3401 * f_n) - 1.0) * atan(1.263 * pow(u / 3.0, 1.629));
    const P_12 = P_9 + (1.0 - P_9) / (1.0 + 1.183 * pow(u, 1.376));
    const P_13 = (1.695 * P_10) / (0.414 + 1.605 * P_10);
    const P_14 = 0.8928 + 0.1072 * (1.0 - exp(-0.42 * pow(f_n / 20.0, 3.215)));
    const P_15 = fabs(1.0 - (0.8928 * (1.0 + P_11) * P_12 * exp(-P_13 * pow(g, 1.092))) / P_14);
    const F_o = P_1 * P_2 * pow((P_3 * P_4 + 0.1844) * f_n * P_15, 1.5763);
    this.er_eff_o = ER - (ER - er_eff) / (1.0 + F_o);
    this.prop_delay_o = UnitPropagationDelay(this.er_eff_o);
  }

  private conductor_losses(): void {
    const e_r_eff_e_0 = this.er_eff_e_0;
    const e_r_eff_o_0 = this.er_eff_o_0;
    const Z0_h_e = this.Z0_e_0 * sqrt(e_r_eff_e_0);
    const Z0_h_o = this.Z0_o_0 * sqrt(e_r_eff_o_0);
    const delta = this.GetParameter("SKIN_DEPTH");
    let alpha_c_e: number;
    let alpha_c_o: number;
    if (this.GetParameter("FREQUENCY") > 0.0) {
      const K = exp(-1.2 * pow((Z0_h_e + Z0_h_o) / (2.0 * ZF0), 0.7));
      let R_s = 1.0 / (this.GetParameter("SIGMA") * delta);
      R_s *= 1.0 + (2.0 / M_PI) * atan(1.4 * pow(this.GetParameter("ROUGH") / delta, 2.0));
      const Q_c_e = (M_PI * Z0_h_e * this.GetParameter("PHYS_WIDTH") * this.GetParameter("FREQUENCY")) / (R_s * C0 * K);
      alpha_c_e = (((20.0 * M_PI) / log(10.0)) * this.GetParameter("FREQUENCY") * sqrt(e_r_eff_e_0)) / (C0 * Q_c_e);
      const Q_c_o = (M_PI * Z0_h_o * this.GetParameter("PHYS_WIDTH") * this.GetParameter("FREQUENCY")) / (R_s * C0 * K);
      alpha_c_o = (((20.0 * M_PI) / log(10.0)) * this.GetParameter("FREQUENCY") * sqrt(e_r_eff_o_0)) / (C0 * Q_c_o);
    } else {
      alpha_c_e = alpha_c_o = 0.0;
    }
    this.atten_cond_e = alpha_c_e * this.GetParameter("PHYS_LEN");
    this.atten_cond_o = alpha_c_o * this.GetParameter("PHYS_LEN");
  }

  private dielectric_losses(): void {
    const e_r = this.GetParameter("EPSILONR");
    const e_r_eff_e_0 = this.er_eff_e_0;
    const e_r_eff_o_0 = this.er_eff_o_0;
    const alpha_d_e = ((20.0 * M_PI) / log(10.0)) * (this.GetParameter("FREQUENCY") / C0) * (e_r / sqrt(e_r_eff_e_0)) * ((e_r_eff_e_0 - 1.0) / (e_r - 1.0)) * this.GetParameter("TAND");
    const alpha_d_o = ((20.0 * M_PI) / log(10.0)) * (this.GetParameter("FREQUENCY") / C0) * (e_r / sqrt(e_r_eff_o_0)) * ((e_r_eff_o_0 - 1.0) / (e_r - 1.0)) * this.GetParameter("TAND");
    this.atten_dielectric_e = alpha_d_e * this.GetParameter("PHYS_LEN");
    this.atten_dielectric_o = alpha_d_o * this.GetParameter("PHYS_LEN");
  }

  private attenuation(): void {
    this.SetParameter("SKIN_DEPTH", this.SkinDepth());
    this.conductor_losses();
    this.dielectric_losses();
  }

  private line_angle(): void {
    const v_e = C0 / sqrt(this.er_eff_e);
    const v_o = C0 / sqrt(this.er_eff_o);
    const lambda_g_e = v_e / this.GetParameter("FREQUENCY");
    const lambda_g_o = v_o / this.GetParameter("FREQUENCY");
    this.ang_l_e = (2.0 * M_PI * this.GetParameter("PHYS_LEN")) / lambda_g_e;
    this.ang_l_o = (2.0 * M_PI * this.GetParameter("PHYS_LEN")) / lambda_g_o;
  }

  /** `diff_impedance`: twice the dispersed odd-mode impedance. */
  private diff_impedance(): void {
    this.Zdiff = 2.0 * this.GetParameter("Z0_O");
  }

  /** `Z0_dispersion`: the frequency dependence of the even- and odd-mode impedances (Kirschning-Jansen 1984, Q_0 .. Q_29). */
  private Z0_dispersion(): void {
    const e_r = this.GetParameter("EPSILONR");
    const u = this.GetParameter("PHYS_WIDTH") / this.GetParameter("H");
    const g = this.GetParameter("PHYS_S") / this.GetParameter("H");
    const f_n = (this.GetParameter("FREQUENCY") * this.GetParameter("H")) / 1e6;
    const e_r_eff_single_f = this.m_aux_microstrip.GetParameter("EPSILON_EFF");
    const e_r_eff_single_0 = this.m_aux_microstrip.er_eff_0;
    const Z0_single_f = this.m_aux_microstrip.GetParameter("Z0");
    const e_r_eff_o_f = this.er_eff_o;
    const e_r_eff_o_0 = this.er_eff_o_0;
    const Q_11 = 0.893 * (1.0 - 0.3 / (1.0 + 0.7 * (e_r - 1.0)));
    const Q_12 = 2.121 * (pow(f_n / 20.0, 4.91) / (1.0 + Q_11 * pow(f_n / 20.0, 4.91))) * exp(-2.87 * g) * pow(g, 0.902);
    const Q_13 = 1.0 + 0.038 * pow(e_r / 8.0, 5.1);
    const Q_14 = 1.0 + (1.203 * pow(e_r / 15.0, 4.0)) / (1.0 + pow(e_r / 15.0, 4.0));
    const Q_15 = (1.887 * exp(-1.5 * pow(g, 0.84)) * pow(g, Q_14)) / (1.0 + (0.41 * pow(f_n / 15.0, 3.0) * pow(u, 2.0 / Q_13)) / (0.125 + pow(u, 1.626 / Q_13)));
    const Q_16 = (1.0 + 9.0 / (1.0 + 0.403 * pow(e_r - 1.0, 2))) * Q_15;
    const Q_17 = 0.394 * (1.0 - exp(-1.47 * pow(u / 7.0, 0.672))) * (1.0 - exp(-4.25 * pow(f_n / 20.0, 1.87)));
    const Q_18 = (0.61 * (1.0 - exp(-2.13 * pow(u / 8.0, 1.593)))) / (1.0 + 6.544 * pow(g, 4.17));
    const Q_19 = (0.21 * g * g * g * g) / ((1.0 + 0.18 * pow(g, 4.9)) * (1.0 + 0.1 * u * u) * (1.0 + pow(f_n / 24.0, 3.0)));
    const Q_20 = (0.09 + 1.0 / (1.0 + 0.1 * pow(e_r - 1, 2.7))) * Q_19;
    const Q_21 = fabs(1.0 - (42.54 * pow(g, 0.133) * exp(-0.812 * g) * pow(u, 2.5)) / (1.0 + 0.033 * pow(u, 2.5)));
    const r_e = pow(f_n / 28.843, 12);
    const q_e = 0.016 + pow(0.0514 * e_r * Q_21, 4.524);
    const p_e = 4.766 * exp(-3.228 * pow(u, 0.641));
    const d_e = 5.086 * q_e * (r_e / (0.3838 + 0.386 * q_e)) * (exp(-22.2 * pow(u, 1.92)) / (1.0 + 1.2992 * r_e)) * (pow(e_r - 1.0, 6.0) / (1.0 + 10 * pow(e_r - 1.0, 6.0)));
    const C_e = 1.0 + 1.275 * (1.0 - exp(-0.004625 * p_e * pow(e_r, 1.674) * pow(f_n / 18.365, 2.745))) - Q_12 + Q_16 - Q_17 + Q_18 + Q_20;
    const R_1 = 0.03891 * pow(e_r, 1.4);
    const R_2 = 0.267 * pow(u, 7.0);
    const R_7 = 1.206 - 0.3144 * exp(-R_1) * (1.0 - exp(-R_2));
    const R_10 = 0.00044 * pow(e_r, 2.136) + 0.0184;
    let tmpf = pow(f_n / 19.47, 6.0);
    const R_11 = tmpf / (1.0 + 0.0962 * tmpf);
    const R_12 = 1.0 / (1.0 + 0.00245 * u * u);
    const R_15 = 0.707 * R_10 * pow(f_n / 12.3, 1.097);
    const R_16 = 1.0 + 0.0503 * e_r * e_r * R_11 * (1.0 - exp(-pow(u / 15.0, 6.0)));
    const Q_0 = R_7 * (1.0 - 1.1241 * (R_12 / R_16) * exp(-0.026 * pow(f_n, 1.15656) - R_15));
    this.SetParameter("Z0_E", (this.Z0_e_0 * pow(0.9408 * pow(e_r_eff_single_f, C_e) - 0.9603, Q_0)) / pow((0.9408 - d_e) * pow(e_r_eff_single_0, C_e) - 0.9603, Q_0));
    const Q_29 = 15.16 / (1.0 + 0.196 * pow(e_r - 1.0, 2.0));
    tmpf = pow(e_r - 1.0, 3.0);
    const Q_28 = (0.149 * tmpf) / (94.5 + 0.038 * tmpf);
    tmpf = pow(e_r - 1.0, 1.5);
    const Q_27 = 0.4 * pow(g, 0.84) * (1.0 + (2.5 * tmpf) / (5.0 + tmpf));
    tmpf = pow((e_r - 1.0) / 13.0, 12.0);
    const Q_26 = 30.0 - 22.2 * (tmpf / (1.0 + 3.0 * tmpf)) - Q_29;
    tmpf = (e_r - 1.0) * (e_r - 1.0);
    const Q_25 = ((0.3 * f_n * f_n) / (10.0 + f_n * f_n)) * (1.0 + (2.333 * tmpf) / (5.0 + tmpf));
    const Q_24 = (2.506 * Q_28 * pow(u, 0.894) * pow(((1.0 + 1.3 * u) * f_n) / 99.25, 4.29)) / (3.575 + pow(u, 0.894));
    const Q_23 = 1.0 + (0.005 * f_n * Q_27) / ((1.0 + 0.812 * pow(f_n / 15.0, 1.9)) * (1.0 + 0.025 * u * u));
    const Q_22 = (0.925 * pow(f_n / Q_26, 1.536)) / (1.0 + 0.3 * pow(f_n / 30.0, 1.536));
    this.SetParameter("Z0_O", Z0_single_f + (this.Z0_o_0 * pow(e_r_eff_o_f / e_r_eff_o_0, Q_22) - Z0_single_f * Q_23) / (1.0 + Q_24 + pow(0.46 * g, 2.2) * Q_25));
  }
}

// ------------------------------------------------------------------------------------------------------------------ COUPLED_STRIPLINE

/** coupled_stripline.{h,cpp} `COUPLED_STRIPLINE` (Cohn 1954 / 1955, with Wadell's off-centre image method). */
export class CoupledStripline extends TranslineCalculationBase {
  private C_f_0 = 0.0;
  private C_f_t_h = 0.0;
  private Z0_w_h_0 = 0.0;
  private Z0_w_h_t_h = 0.0;
  private Z0_e_w_h_0_s_h = 0.0;
  private Z0_o_w_h_0_s_h = 0.0;
  private e_eff_e = 0.0;
  private e_eff_o = 0.0;
  private ang_l = 0.0;
  private unit_prop_delay_e = 0.0;
  private unit_prop_delay_o = 0.0;
  private m_striplineCalc = new Stripline();

  constructor() {
    super(["SKIN_DEPTH", "Z0_E", "Z0_O", "Z_DIFF", "Z_COMM", "COUPLING_K", "PHYS_WIDTH", "FREQUENCY", "PHYS_LEN", "H", "PHYS_S", "T", "EPSILONR", "MUR", "MURC", "SIGMA", "TAND", "ROUGH", "ANG_L", "STRIPLINE_A", "ATTEN_COND_EVEN", "ATTEN_COND_ODD", "ATTEN_DILECTRIC_EVEN", "ATTEN_DILECTRIC_ODD", "DIELECTRIC_MODEL_SEL", "EPSILONR_SPEC_FREQ"]);
  }

  /** `COUPLED_STRIPLINE::Analyse`. */
  Analyse(): void {
    this.UpdateDielectricModel();
    const f = this.GetParameter("FREQUENCY");
    const rawEpsR = this.GetParameter("EPSILONR");
    const rawTanD = this.GetParameter("TAND");
    this.SetParameter("EPSILONR", this.GetDispersedEpsilonR(f));
    this.SetParameter("TAND", this.GetDispersedTanDelta(f));
    this.SetParameter("SKIN_DEPTH", this.SkinDepth());
    const w = this.GetParameter("PHYS_WIDTH");
    const t = this.GetParameter("T");
    const s = this.GetParameter("PHYS_S");
    const h = this.GetParameter("H");
    let a = this.GetParameter("STRIPLINE_A");
    const er = this.GetParameter("EPSILONR");
    if (a <= 0.0) a = h / 2.0;
    this.calcOffsetZeroThicknessCoupledImpedances(h, a, w, s, t, er);
    if (t === 0.0) {
      this.SetParameter("Z0_E", this.Z0_e_w_h_0_s_h);
      this.SetParameter("Z0_O", this.Z0_o_w_h_0_s_h);
    } else if (CoupledStripline.IsCenteredOffset(a, h)) {
      this.calcSingleStripImpedances();
      this.calcFringeCapacitances(h, t, er);
      this.calcZ0EvenMode();
      this.calcZ0OddMode(t, s);
    } else if (!CoupledStripline.isOffsetWithinFiniteThicknessLimits(a, h, t)) {
      this.SetParameter("Z0_E", Number.NaN);
      this.SetParameter("Z0_O", Number.NaN);
      this.SetParameter("Z_DIFF", Number.NaN);
    } else {
      const [z0e_1, z0o_1] = this.calcOffsetVirtualBranch(2.0 * a, w, s, t, er);
      const [z0e_2, z0o_2] = this.calcOffsetVirtualBranch(2.0 * (h - a), w, s, t, er);
      this.SetParameter("H", h);
      const z0e_image = 2.0 / (1.0 / z0e_1 + 1.0 / z0e_2);
      const z0o_image = 2.0 / (1.0 / z0o_1 + 1.0 / z0o_2);
      const z0e = CoupledStripline.applyOffsetCorrection(z0e_image, a, h, w, t, er);
      const z0o = CoupledStripline.applyOffsetCorrection(z0o_image, a, h, w, t, er);
      this.SetParameter("Z0_E", z0e);
      this.SetParameter("Z0_O", z0o);
      this.SetParameter("Z_DIFF", 2.0 * z0o);
      this.calcOffsetZeroThicknessCoupledImpedances(h, a, w, s, t, er);
    }
    this.calcLosses();
    this.calcDielectrics();
    const z0e = this.GetParameter("Z0_E");
    const z0o = this.GetParameter("Z0_O");
    this.SetParameter("Z_COMM", 0.5 * z0e);
    if (z0e + z0o > 0.0) this.SetParameter("COUPLING_K", (z0e - z0o) / (z0e + z0o));
    else this.SetParameter("COUPLING_K", 0.0);
    this.SetParameter("EPSILONR", rawEpsR);
    this.SetParameter("TAND", rawTanD);
  }

  /**
   * `COUPLED_STRIPLINE::Synthesize`: FROM_ZDIFF_ZCOMM first turns the Z_DIFF / Z_COMM target into Z0_O = Z_DIFF / 2 and Z0_E = 2 Z_COMM (false when
   * either is not positive); FIX_WIDTH solves the gap, FIX_SPACING the width, each on the odd-mode impedance; DEFAULT (and FROM_ZDIFF_ZCOMM) solves
   * width and gap together, wcalc's 2-D Newton with a finite-difference Jacobian and steps capped at 10 %, at most 50 iterations.
   */
  Synthesize(aOpts: SynthesizeOpts): boolean {
    if (aOpts === "FROM_ZDIFF_ZCOMM") {
      const zDiffTarget = this.GetParameter("Z_DIFF");
      const zCommTarget = this.GetParameter("Z_COMM");
      if (!(zDiffTarget > 0.0) || !(zCommTarget > 0.0)) return false;
      this.SetParameter("Z0_O", 0.5 * zDiffTarget);
      this.SetParameter("Z0_E", 2.0 * zCommTarget);
    }
    if (aOpts === "FIX_WIDTH") return this.MinimiseZ0Error1D("PHYS_S", "Z0_O", false);
    if (aOpts === "FIX_SPACING") return this.MinimiseZ0Error1D("PHYS_WIDTH", "Z0_O", false);
    let ze0 = 0;
    let zo0 = 0;
    const h = this.GetParameter("H");
    const er = this.GetParameter("EPSILONR");
    const z0e_target = this.GetParameter("Z0_E");
    const z0o_target = this.GetParameter("Z0_O");
    const z0 = sqrt(z0e_target * z0o_target);
    const k = (z0e_target - z0o_target) / (z0e_target + z0o_target);
    const maxiters = 50;
    const ai = [1, -0.301, 3.209, -27.282, 56.609, -37.746];
    const bi = [0.02, -0.623, 17.192, -68.946, 104.74, -16.148];
    const ci = [0.002, -0.347, 7.171, -36.91, 76.132, -51.616];
    const AW = exp((z0 * sqrt(er + 1.0)) / 42.4) - 1.0;
    const F1 = (8.0 * sqrt((AW * (7.0 + 4.0 / er)) / 11.0 + (1.0 + 1.0 / er) / 0.81)) / AW;
    let F2 = 0.0;
    let F3 = 0.0;
    for (let i = 0; i <= 5; i++) F2 = F2 + ai[i]! * pow(k, i);
    for (let i = 0; i <= 5; i++) F3 = F3 + (bi[i]! - ci[i]! * (9.6 - er)) * pow(0.6 - k, i);
    let w = h * fabs(F1 * F2);
    let s = h * fabs(F1 * F3);
    let iters = 0;
    let done = false;
    const delta = UNIT_MIL * 1e-5;
    const cval = 1e-12 * z0e_target * z0o_target;
    while (!done && iters < maxiters) {
      iters++;
      this.SetParameter("PHYS_WIDTH", w);
      this.SetParameter("PHYS_S", s);
      this.Analyse();
      ze0 = this.GetParameter("Z0_E");
      zo0 = this.GetParameter("Z0_O");
      const err = pow(ze0 - z0e_target, 2.0) + pow(zo0 - z0o_target, 2.0);
      if (err < cval) {
        done = true;
      } else {
        this.SetParameter("PHYS_WIDTH", w + delta);
        this.SetParameter("PHYS_S", s);
        this.Analyse();
        const ze1 = this.GetParameter("Z0_E");
        const zo1 = this.GetParameter("Z0_O");
        this.SetParameter("PHYS_WIDTH", w);
        this.SetParameter("PHYS_S", s + delta);
        this.Analyse();
        const ze2 = this.GetParameter("Z0_E");
        const zo2 = this.GetParameter("Z0_O");
        const dedw = (ze1 - ze0) / delta;
        const dodw = (zo1 - zo0) / delta;
        const deds = (ze2 - ze0) / delta;
        const dods = (zo2 - zo0) / delta;
        const d = dedw * dods - deds * dodw;
        let dw = (-1.0 * ((ze0 - z0e_target) * dods - (zo0 - z0o_target) * deds)) / d;
        if (fabs(dw) > 0.1 * w) dw = dw > 0.0 ? 0.1 * w : -0.1 * w;
        w = fabs(w + dw);
        let ds = ((ze0 - z0e_target) * dodw - (zo0 - z0o_target) * dedw) / d;
        if (fabs(ds) > 0.1 * s) ds = ds > 0.0 ? 0.1 * s : -0.1 * s;
        s = fabs(s + ds);
      }
    }
    if (!done) return false;
    this.SetParameter("PHYS_WIDTH", w);
    this.SetParameter("PHYS_S", s);
    this.Analyse();
    this.SetParameter("Z0_E", z0e_target);
    this.SetParameter("Z0_O", z0o_target);
    return true;
  }

  /** `publishResults`. */
  private publishResults(set: (p: TCP, v: number, s: TranslineStatus) => void, aImpedanceFailure: TranslineStatus, aGeometryFailure: TranslineStatus): void {
    const writeOk = (p: TCP, v: number) => set(p, v, "OK");
    const writeChecked = (p: TCP, v: number, positive: boolean, failure: TranslineStatus) => {
      const invalid = !isfinite(v) || (positive ? v <= 0.0 : v < 0.0);
      set(p, v, invalid ? failure : "OK");
    };
    writeOk("EPSILON_EFF_EVEN", this.e_eff_e);
    writeOk("EPSILON_EFF_ODD", this.e_eff_o);
    writeOk("UNIT_PROP_DELAY_EVEN", this.unit_prop_delay_e);
    writeOk("UNIT_PROP_DELAY_ODD", this.unit_prop_delay_o);
    writeOk("SKIN_DEPTH", this.GetParameter("SKIN_DEPTH"));
    writeOk("ATTEN_COND_EVEN", this.GetParameter("ATTEN_COND_EVEN"));
    writeOk("ATTEN_COND_ODD", this.GetParameter("ATTEN_COND_ODD"));
    writeOk("ATTEN_DILECTRIC_EVEN", this.GetParameter("ATTEN_DILECTRIC_EVEN"));
    writeOk("ATTEN_DILECTRIC_ODD", this.GetParameter("ATTEN_DILECTRIC_ODD"));
    writeChecked("Z0_E", this.GetParameter("Z0_E"), true, aImpedanceFailure);
    writeChecked("Z0_O", this.GetParameter("Z0_O"), true, aImpedanceFailure);
    writeChecked("Z_DIFF", this.GetParameter("Z_DIFF"), true, aImpedanceFailure);
    writeOk("Z_COMM", this.GetParameter("Z_COMM"));
    writeOk("COUPLING_K", this.GetParameter("COUPLING_K"));
    writeChecked("ANG_L", this.ang_l, false, aImpedanceFailure);
    writeChecked("PHYS_WIDTH", this.GetParameter("PHYS_WIDTH"), true, aGeometryFailure);
    writeChecked("PHYS_LEN", this.GetParameter("PHYS_LEN"), false, aGeometryFailure);
    writeChecked("PHYS_S", this.GetParameter("PHYS_S"), true, aGeometryFailure);
  }

  protected SetAnalysisResults(): void {
    this.publishResults((p, v, s) => this.SetAnalysisResult(p, v, s), "TS_ERROR", "WARNING");
  }

  protected SetSynthesisResults(): void {
    this.publishResults((p, v, s) => this.SetSynthesisResult(p, v, s), "WARNING", "TS_ERROR");
  }

  /** `calcFringeCapacitances`: Cohn 1954 Eq. 2 and Cohn 1955 Eq. 13. */
  private calcFringeCapacitances(h: number, t: number, er: number): void {
    this.C_f_t_h = ((E0 * er) / M_PI) * ((2.0 / (1.0 - t / h)) * log(1.0 / (1.0 - t / h) + 1.0) - (1.0 / (1.0 - t / h) - 1.0) * log(1.0 / pow(1.0 - t / h, 2.0) - 1.0));
    this.C_f_0 = ((E0 * er) / M_PI) * 2.0 * log(2.0);
  }

  /** `calcZeroThicknessCoupledImpedances`: Cohn 1955 Eqs. 2 - 7. */
  private calcZeroThicknessCoupledImpedances(h: number, w: number, s: number, er: number): void {
    const EI = TranslineCalculationBase.EllipticIntegral;
    const k_e = tanh((M_PI * w) / (2.0 * h)) * tanh((M_PI * (w + s)) / (2.0 * h));
    const k_o = tanh((M_PI * w) / (2.0 * h)) * TranslineCalculationBase.coth((M_PI * (w + s)) / (2.0 * h));
    const k_e_p = sqrt(1 - pow(k_e, 2));
    const k_o_p = sqrt(1 - pow(k_o, 2));
    this.Z0_e_w_h_0_s_h = (ZF0 / (4.0 * sqrt(er))) * (EI(k_e_p)[0] / EI(k_e)[0]);
    this.Z0_o_w_h_0_s_h = (ZF0 / (4.0 * sqrt(er))) * (EI(k_o_p)[0] / EI(k_o)[0]);
  }

  /** `IsCenteredOffset`: the strip plane is at h/2 within 1e-9 h. */
  static IsCenteredOffset(a: number, h: number): boolean {
    return fabs(a - h / 2.0) <= h * 1e-9;
  }

  private static isOffsetWithinFiniteThicknessLimits(a: number, h: number, t: number): boolean {
    return a > 0.5 * t && a < h - 0.5 * t;
  }

  private calcOffsetVirtualBranch(aVirtualH: number, w: number, s: number, t: number, er: number): [number, number] {
    this.SetParameter("H", aVirtualH);
    this.calcZeroThicknessCoupledImpedances(aVirtualH, w, s, er);
    this.calcSingleStripImpedances();
    this.calcFringeCapacitances(aVirtualH, t, er);
    this.calcZ0EvenMode();
    this.calcZ0OddMode(t, s);
    return [this.GetParameter("Z0_E"), this.GetParameter("Z0_O")];
  }

  private calcOffsetZeroThicknessCoupledImpedances(h: number, a: number, w: number, s: number, t: number, er: number): void {
    if (CoupledStripline.IsCenteredOffset(a, h)) {
      this.calcZeroThicknessCoupledImpedances(h, w, s, er);
      return;
    }
    this.calcZeroThicknessCoupledImpedances(2.0 * a, w, s, er);
    const z0e_1 = this.Z0_e_w_h_0_s_h;
    const z0o_1 = this.Z0_o_w_h_0_s_h;
    this.calcZeroThicknessCoupledImpedances(2.0 * (h - a), w, s, er);
    const z0e_2 = this.Z0_e_w_h_0_s_h;
    const z0o_2 = this.Z0_o_w_h_0_s_h;
    const z0e_image = 2.0 / (1.0 / z0e_1 + 1.0 / z0e_2);
    const z0o_image = 2.0 / (1.0 / z0o_1 + 1.0 / z0o_2);
    this.Z0_e_w_h_0_s_h = CoupledStripline.applyOffsetCorrection(z0e_image, a, h, w, t, er);
    this.Z0_o_w_h_0_s_h = CoupledStripline.applyOffsetCorrection(z0o_image, a, h, w, t, er);
  }

  /** `applyOffsetCorrection`: Wadell 1991 Eq. 3.6.3.23. */
  private static applyOffsetCorrection(aZImage: number, aOffset: number, aPlateSpacing: number, aWidth: number, aThickness: number, aEr: number): number {
    const position = fabs(0.5 - aOffset / aPlateSpacing);
    const positionFactor = pow(position, 2.2);
    const widthFactor = pow((aThickness + aWidth) / aPlateSpacing, 2.9);
    const correction = ((0.26 * M_PI) / 8.0) * sqrt(aEr) * aZImage * positionFactor * widthFactor;
    return aZImage * (1.0 - correction);
  }

  /** `calcSingleStripImpedances`: the finite- and zero-thickness single strips (Cohn 1954 Eqs. 5 - 6). */
  private calcSingleStripImpedances(): void {
    const er = this.GetParameter("EPSILONR");
    const h = this.GetParameter("H");
    const w = this.GetParameter("PHYS_WIDTH");
    this.Z0_w_h_t_h = this.calcZ0SymmetricStripline();
    const k = TranslineCalculationBase.sech((M_PI * w) / (2.0 * h));
    const k_p = tanh((M_PI * w) / (2.0 * h));
    const EI = TranslineCalculationBase.EllipticIntegral;
    this.Z0_w_h_0 = (ZF0 / (4.0 * sqrt(er))) * (EI(k)[0] / EI(k_p)[0]);
  }

  /** `calcZ0EvenMode`: Cohn 1955 Eq. 18. */
  private calcZ0EvenMode(): void {
    const Z_e = 1.0 / (1.0 / this.Z0_w_h_t_h - (this.C_f_t_h / this.C_f_0) * (1.0 / this.Z0_w_h_0 - 1.0 / this.Z0_e_w_h_0_s_h));
    this.SetParameter("Z0_E", Z_e);
  }

  /** `calcZ0OddMode`: Cohn 1955 Eq. 20 (s/t >= 5) or Eq. 22. */
  private calcZ0OddMode(t: number, s: number): void {
    const Z_o_1 = 1.0 / (1.0 / this.Z0_w_h_t_h + (this.C_f_t_h / this.C_f_0) * (1.0 / this.Z0_o_w_h_0_s_h - 1.0 / this.Z0_w_h_0));
    const Z_o_2 = 1.0 / (1.0 / this.Z0_o_w_h_0_s_h + (1.0 / this.Z0_w_h_t_h - 1.0 / this.Z0_w_h_0) - (2.0 / ZF0) * (this.C_f_t_h / E0 - this.C_f_0 / E0) + (2.0 * t) / (ZF0 * s));
    const Z_o = s / t >= 5.0 ? Z_o_1 : Z_o_2;
    this.SetParameter("Z0_O", Z_o);
    this.SetParameter("Z_DIFF", 2.0 * Z_o);
  }

  private setInnerStripline(tand: number, angL: number): void {
    const m = this.m_striplineCalc;
    m.SetParameter("EPSILONR", this.GetParameter("EPSILONR"));
    m.SetParameter("T", this.GetParameter("T"));
    m.SetParameter("STRIPLINE_A", this.GetParameter("H") / 2.0);
    m.SetParameter("H", this.GetParameter("H"));
    m.SetParameter("PHYS_LEN", this.GetParameter("PHYS_LEN"));
    m.SetParameter("FREQUENCY", this.GetParameter("FREQUENCY"));
    m.SetParameter("TAND", tand);
    m.SetParameter("PHYS_WIDTH", this.GetParameter("PHYS_WIDTH"));
    m.SetParameter("ANG_L", angL);
    m.SetParameter("SIGMA", this.GetParameter("SIGMA"));
    m.SetParameter("MURC", this.GetParameter("MURC"));
    m.Analyse();
  }

  /** `calcLosses`: the homogeneous-TEM dielectric loss and the single stripline's conductor loss scaled to each mode's impedance. */
  private calcLosses(): void {
    const length = this.GetParameter("PHYS_LEN");
    const freq = this.GetParameter("FREQUENCY");
    const er = this.GetParameter("EPSILONR");
    const tand = this.GetParameter("TAND");
    const alpha_d_dB_per_m = LOG2DB * (M_PI / C0) * freq * sqrt(er) * tand;
    const atten_d = alpha_d_dB_per_m * length;
    this.SetParameter("ATTEN_DILECTRIC_EVEN", atten_d);
    this.SetParameter("ATTEN_DILECTRIC_ODD", atten_d);
    // the C++ sets EPSILONR from its local `er`, the same value
    this.setInnerStripline(0.0, 0.0);
    const loss_single = this.m_striplineCalc.GetParameter("LOSS_CONDUCTOR");
    const z0_single = this.m_striplineCalc.GetParameter("Z0");
    const z0_e = this.GetParameter("Z0_E");
    const z0_o = this.GetParameter("Z0_O");
    let atten_c_e = 0.0;
    let atten_c_o = 0.0;
    if (z0_e > 0.0 && isfinite(z0_e)) atten_c_e = loss_single * (z0_single / z0_e);
    if (z0_o > 0.0 && isfinite(z0_o)) atten_c_o = loss_single * (z0_single / z0_o);
    this.SetParameter("ATTEN_COND_EVEN", atten_c_e);
    this.SetParameter("ATTEN_COND_ODD", atten_c_o);
  }

  /** `calcDielectrics`: a homogeneous dielectric, so both modes see e_r. */
  private calcDielectrics(): void {
    const e_r = this.GetParameter("EPSILONR");
    this.e_eff_e = e_r;
    this.e_eff_o = e_r;
    const unitPropDelay = UnitPropagationDelay(e_r);
    this.unit_prop_delay_e = unitPropDelay;
    this.unit_prop_delay_o = unitPropDelay;
    const v = C0 / sqrt(e_r);
    const lambda_g = v / this.GetParameter("FREQUENCY");
    this.ang_l = (2.0 * M_PI * this.GetParameter("PHYS_LEN")) / lambda_g;
  }

  /** `calcZ0SymmetricStripline`: the centred single stripline of this width and thickness. */
  private calcZ0SymmetricStripline(): number {
    this.setInnerStripline(0.0, 0);
    return this.m_striplineCalc.GetParameter("Z0");
  }
}

// ------------------------------------------------------------------------------------------------------------------ COAX

/** The status helpers the four calculators below share (`x ? TRANSLINE_STATUS::TS_ERROR : TRANSLINE_STATUS::OK` and the like). */
const st = (bad: boolean, s: TranslineStatus): TranslineStatus => (bad ? s : "OK");

/**
 * coax.{h,cpp} `COAX`: a coaxial line, Pozar section 2.2 (Z0) and 3.5 (higher-order modes). Synthesis solves the inner or the outer diameter in closed
 * form (`SetSynthesizeTarget`, the inner one by default) and the length from the angular length. `GetTEModes` / `GetTMModes` are the propagating modes
 * the panel shows ("H(1,1) ", "E(0,1) ", ...).
 */
export class Coax extends TranslineCalculationBase {
  private m_teModes = "";
  private m_tmModes = "";

  constructor() {
    super(["SKIN_DEPTH", "EPSILONR", "TAND", "PHYS_DIAM_IN", "PHYS_DIAM_OUT", "MUR", "MURC", "SIGMA", "FREQUENCY", "PHYS_LEN", "Z0", "ANG_L", "LOSS_CONDUCTOR", "LOSS_DIELECTRIC", "CUTOFF_FREQUENCY", "DIELECTRIC_MODEL_SEL", "EPSILONR_SPEC_FREQ"]);
    this.m_synthesizeTarget = "PHYS_DIAM_IN";
  }

  override SetSynthesizeTarget(aTarget: TCP): void {
    if (aTarget === "PHYS_DIAM_IN" || aTarget === "PHYS_DIAM_OUT") this.m_synthesizeTarget = aTarget;
  }

  GetTEModes(): string {
    return this.m_teModes;
  }

  GetTMModes(): string {
    return this.m_tmModes;
  }

  /** `AlphaD`: homogeneous-TEM dielectric loss, dB/m. */
  private AlphaD(): number {
    const ad = (M_PI / C0) * this.GetParameter("FREQUENCY") * sqrt(this.GetParameter("EPSILONR")) * this.GetParameter("TAND");
    return ad * LOG2DB;
  }

  /** `AlphaC`: conductor loss, dB/m. */
  private AlphaC(): number {
    const Rs = sqrt((M_PI * this.GetParameter("FREQUENCY") * this.GetParameter("MURC") * MU0) / this.GetParameter("SIGMA"));
    const Din = this.GetParameter("PHYS_DIAM_IN");
    const Dout = this.GetParameter("PHYS_DIAM_OUT");
    const ac = ((sqrt(this.GetParameter("EPSILONR")) * (1.0 / Din + 1.0 / Dout)) / log(Dout / Din)) * (Rs / ZF0);
    return ac * LOG2DB;
  }

  /** `UpdateModeCutoffs`: TE11's cutoff (CUTOFF_FREQUENCY) and the TE_1m / TM_0m modes below the frequency. */
  private UpdateModeCutoffs(): void {
    const epsr = this.GetParameter("EPSILONR");
    const mur = this.GetParameter("MUR");
    const Din = this.GetParameter("PHYS_DIAM_IN");
    const Dout = this.GetParameter("PHYS_DIAM_OUT");
    const freq = this.GetParameter("FREQUENCY");
    const sqrtMuEr = sqrt(epsr * mur);
    const fc_TE11 = (2.0 * C0) / (M_PI * sqrtMuEr * (Din + Dout));
    const fc_step = C0 / (sqrtMuEr * (Dout - Din));
    this.SetParameter("CUTOFF_FREQUENCY", fc_TE11);
    this.m_teModes = "";
    this.m_tmModes = "";
    if (fc_TE11 <= freq) {
      this.m_teModes = "H(1,1) ";
      for (let m = 2; m < 10; ++m) {
        const fc = fc_TE11 + (m - 1) * fc_step;
        if (fc > freq) break;
        this.m_teModes += `H(1,${m}) `;
      }
    }
    for (let m = 1; m < 10; ++m) {
      const fc = m * fc_step;
      if (fc > freq) break;
      this.m_tmModes += `E(0,${m}) `;
    }
  }

  /** `COAX::Analyse`. */
  Analyse(): void {
    this.UpdateDielectricModel();
    const freq = this.GetParameter("FREQUENCY");
    const rawEpsR = this.GetParameter("EPSILONR");
    const rawTanD = this.GetParameter("TAND");
    this.SetParameter("EPSILONR", this.GetDispersedEpsilonR(freq));
    this.SetParameter("TAND", this.GetDispersedTanDelta(freq));
    this.SetParameter("SKIN_DEPTH", this.SkinDepth());
    const Din = this.GetParameter("PHYS_DIAM_IN");
    const Dout = this.GetParameter("PHYS_DIAM_OUT");
    const epsr = this.GetParameter("EPSILONR");
    const mur = this.GetParameter("MUR");
    const len = this.GetParameter("PHYS_LEN");
    this.SetParameter("Z0", (ZF0 / (2.0 * M_PI * sqrt(epsr))) * log(Dout / Din));
    const lambda_g = C0 / freq / sqrt(epsr * mur);
    this.SetParameter("ANG_L", (2.0 * M_PI * len) / lambda_g);
    this.SetParameter("LOSS_DIELECTRIC", this.AlphaD() * len);
    this.SetParameter("LOSS_CONDUCTOR", this.AlphaC() * len);
    this.UpdateModeCutoffs();
    this.SetParameter("EPSILONR", rawEpsR);
    this.SetParameter("TAND", rawTanD);
  }

  /** `COAX::Synthesize`: the target diameter from Z0, the length from ANG_L, then `Analyse` with Z0 and ANG_L put back. Always true. */
  Synthesize(_aOpts: SynthesizeOpts): boolean {
    this.UpdateDielectricModel();
    const mur = this.GetParameter("MUR");
    const freq = this.GetParameter("FREQUENCY");
    const Z0 = this.GetParameter("Z0");
    const angL = this.GetParameter("ANG_L");
    const epsr = this.GetDispersedEpsilonR(freq);
    const k = ((Z0 * sqrt(epsr)) / ZF0) * 2.0 * M_PI;
    if (this.m_synthesizeTarget === "PHYS_DIAM_IN") this.SetParameter("PHYS_DIAM_IN", this.GetParameter("PHYS_DIAM_OUT") / exp(k));
    else if (this.m_synthesizeTarget === "PHYS_DIAM_OUT") this.SetParameter("PHYS_DIAM_OUT", this.GetParameter("PHYS_DIAM_IN") * exp(k));
    const lambda_g = C0 / freq / sqrt(epsr * mur);
    this.SetParameter("PHYS_LEN", (lambda_g * angL) / (2.0 * M_PI));
    this.Analyse();
    this.SetParameter("Z0", Z0);
    this.SetParameter("ANG_L", angL);
    return true;
  }

  protected SetAnalysisResults(): void {
    for (const p of ["LOSS_CONDUCTOR", "LOSS_DIELECTRIC", "EPSILONR", "SKIN_DEPTH", "CUTOFF_FREQUENCY"] as const) this.SetAnalysisResult(p, this.GetParameter(p));
    const Z0 = this.GetParameter("Z0");
    const angL = this.GetParameter("ANG_L");
    const len = this.GetParameter("PHYS_LEN");
    const Din = this.GetParameter("PHYS_DIAM_IN");
    const Dout = this.GetParameter("PHYS_DIAM_OUT");
    const geometry_invalid = Din >= Dout;
    this.SetAnalysisResult("Z0", Z0, st(!isfinite(Z0) || Z0 < 0, "TS_ERROR"));
    this.SetAnalysisResult("ANG_L", angL, st(!isfinite(angL) || angL < 0, "TS_ERROR"));
    this.SetAnalysisResult("PHYS_LEN", len, st(!isfinite(len) || len < 0, "WARNING"));
    this.SetAnalysisResult("PHYS_DIAM_IN", Din, st(!isfinite(Din) || Din <= 0.0 || geometry_invalid, "WARNING"));
    this.SetAnalysisResult("PHYS_DIAM_OUT", Dout, st(!isfinite(Dout) || Dout <= 0.0 || geometry_invalid, "WARNING"));
  }

  protected SetSynthesisResults(): void {
    for (const p of ["LOSS_CONDUCTOR", "LOSS_DIELECTRIC", "EPSILONR", "SKIN_DEPTH", "CUTOFF_FREQUENCY"] as const) this.SetSynthesisResult(p, this.GetParameter(p));
    const Z0 = this.GetParameter("Z0");
    const angL = this.GetParameter("ANG_L");
    const len = this.GetParameter("PHYS_LEN");
    const Din = this.GetParameter("PHYS_DIAM_IN");
    const Dout = this.GetParameter("PHYS_DIAM_OUT");
    const Din_invalid = !isfinite(Din) || Din <= 0.0;
    const Dout_invalid = !isfinite(Dout) || Dout <= 0.0;
    const geometry_invalid = Din >= Dout;
    const tIn = this.m_synthesizeTarget === "PHYS_DIAM_IN";
    const tOut = this.m_synthesizeTarget === "PHYS_DIAM_OUT";
    const Din_status = st(Din_invalid, tIn ? "TS_ERROR" : "WARNING");
    const Dout_status = st(Dout_invalid, tOut ? "TS_ERROR" : "WARNING");
    this.SetSynthesisResult("Z0", Z0, st(!isfinite(Z0) || Z0 < 0, "WARNING"));
    this.SetSynthesisResult("ANG_L", angL, st(!isfinite(angL) || angL < 0, "WARNING"));
    this.SetSynthesisResult("PHYS_LEN", len, st(!isfinite(len) || len < 0, "TS_ERROR"));
    this.SetSynthesisResult("PHYS_DIAM_IN", Din, geometry_invalid ? (tIn ? "TS_ERROR" : "WARNING") : Din_status);
    this.SetSynthesisResult("PHYS_DIAM_OUT", Dout, geometry_invalid ? (tOut ? "TS_ERROR" : "WARNING") : Dout_status);
  }
}

// ------------------------------------------------------------------------------------------------------------------ COPLANAR

/**
 * coplanar.{h,cpp} `COPLANAR`: a coplanar waveguide, and with CPW_BACKMETAL >= 0.5 the conductor-backed one (Ghione-Naldi conformal mapping, Gupta's
 * thickness correction, Gevorgian's dispersion, Owyang-Wu / Ghione conductor loss). Synthesis solves the width or the gap (`SetSynthesizeTarget`, the
 * width by default) by `MinimiseZ0Error1D`, recomputing the length from the angular length.
 */
export class Coplanar extends TranslineCalculationBase {
  constructor() {
    super(["EPSILONR", "TAND", "SIGMA", "MURC", "PHYS_WIDTH", "PHYS_S", "H", "T", "PHYS_LEN", "FREQUENCY", "Z0", "ANG_L", "LOSS_CONDUCTOR", "LOSS_DIELECTRIC", "SKIN_DEPTH", "EPSILON_EFF", "UNIT_PROP_DELAY", "CPW_BACKMETAL", "DIELECTRIC_MODEL_SEL", "EPSILONR_SPEC_FREQ", "SOLDERMASK_PRESENT", "SOLDERMASK_THICKNESS", "SOLDERMASK_EPSILONR", "SOLDERMASK_TAND", "SOLDERMASK_FILLS_GAPS"]);
    this.m_synthesizeTarget = "PHYS_WIDTH";
  }

  override SetSynthesizeTarget(aTarget: TCP): void {
    if (aTarget === "PHYS_WIDTH" || aTarget === "PHYS_S") this.m_synthesizeTarget = aTarget;
  }

  private hasBackMetal(): boolean {
    return this.GetParameter("CPW_BACKMETAL") >= 0.5;
  }

  /** `COPLANAR::GetSoldermaskDeltaQ`: the microstrip increment, times the upper half-space share (0.5, 0.25 with back metal) and the slot coverage (1, 0.4 with air slots). */
  override GetSoldermaskDeltaQ(aWOverH: number, aCOverH: number): number {
    if (aWOverH <= 0.0 || aCOverH <= 0.0) return 0.0;
    const q2Coated = WanHoorfarQ2(aWOverH, 1.0 + aCOverH);
    const q2Base = WanHoorfarQ2(aWOverH, 1.0);
    const microstripDelta = Math.max(0.0, q2Coated - q2Base);
    const fillsGaps = this.GetParameter("SOLDERMASK_FILLS_GAPS") >= 0.5;
    const halfSpace = this.hasBackMetal() ? 0.25 : 0.5;
    const slotCoverage = fillsGaps ? 1.0 : 0.4;
    return halfSpace * slotCoverage * microstripDelta;
  }

  /** `COPLANAR::Analyse`. */
  Analyse(): void {
    this.UpdateDielectricModel();
    this.SetParameter("SKIN_DEPTH", this.SkinDepth());
    const W = this.GetParameter("PHYS_WIDTH");
    const S = this.GetParameter("PHYS_S");
    const H = this.GetParameter("H");
    const T = this.GetParameter("T");
    const freq = this.GetParameter("FREQUENCY");
    const epsr = this.GetDispersedEpsilonR(freq);
    const len = this.GetParameter("PHYS_LEN");
    const tand_substrate = this.GetDispersedTanDelta(freq);
    const sigma = this.GetParameter("SIGMA");
    const backMetal = this.hasBackMetal();
    const K = (x: number) => TranslineCalculationBase.EllipticIntegral(x)[0];
    const k1 = W / (W + S + S);
    const kk1 = K(k1);
    const kpk1 = K(sqrt(1.0 - k1 * k1));
    const q1 = kk1 / kpk1;
    let q3 = 0.0;
    let qz = 0.0;
    let er0 = 0.0;
    let zl_factor = 0.0;
    if (backMetal) {
      const k3 = tanh((M_PI / 4.0) * (W / H)) / tanh(((M_PI / 4.0) * (W + S + S)) / H);
      q3 = K(k3) / K(sqrt(1.0 - k3 * k3));
      qz = 1.0 / (q1 + q3);
      er0 = 1.0 + q3 * qz * (epsr - 1.0);
      zl_factor = (ZF0 / 2.0) * qz;
    } else {
      const k2 = sinh((M_PI / 4.0) * (W / H)) / sinh(((M_PI / 4.0) * (W + S + S)) / H);
      const q2 = K(k2) / K(sqrt(1.0 - k2 * k2));
      er0 = 1.0 + (((epsr - 1.0) / 2.0) * q2) / q1;
      zl_factor = ZF0 / 4.0 / q1;
    }
    if (T > 0.0) {
      const d = ((T * 1.25) / M_PI) * (1.0 + log((4.0 * M_PI * W) / T));
      const se = S - d;
      const We = W + d;
      const ke = We / (We + se + se);
      const qe = K(ke) / K(sqrt(1.0 - ke * ke));
      if (backMetal) {
        qz = 1.0 / (qe + q3);
        er0 = 1.0 + q3 * qz * (epsr - 1.0);
        zl_factor = (ZF0 / 2.0) * qz;
      } else {
        zl_factor = ZF0 / 4.0 / qe;
      }
      er0 = er0 - (0.7 * (er0 - 1.0) * T) / S / (q1 + (0.7 * T) / S);
    }
    let tand_eff = tand_substrate;
    const uOverH = H > 0.0 ? W / H : 0.0;
    const [er0_coated, tand_coated] = this.ApplySoldermaskCorrection(er0, tand_substrate, epsr, uOverH, freq);
    if (er0_coated !== er0) {
      zl_factor *= sqrt(er0 / er0_coated);
      er0 = er0_coated;
      tand_eff = tand_coated;
    }
    const sr_er = sqrt(epsr);
    const sr_er0 = sqrt(er0);
    const fte = C0 / 4.0 / (H * sqrt(epsr - 1.0));
    const p = log(W / H);
    const u = 0.54 - (0.64 - 0.015 * p) * p;
    const v = 0.43 - (0.86 - 0.54 * p) * p;
    const G = exp(u * log(W / S) + v);
    let ac = 0.0;
    if (T > 0.0) {
      const n = ((1.0 - k1) * 8.0 * M_PI) / (T * (1.0 + k1));
      const a = W / 2.0;
      const b = a + S;
      ac = (M_PI + log(n * a)) / a + (M_PI + log(n * b)) / b;
    }
    const ac_factor = ac / (4.0 * ZF0 * kk1 * kpk1 * (1.0 - k1 * k1));
    const ad_factor = ((epsr / (epsr - 1.0)) * tand_eff * M_PI) / C0;
    let sr_er_f = sr_er0;
    sr_er_f += (sr_er - sr_er0) / (1.0 + G * pow(freq / fte, -1.8));
    this.SetParameter("LOSS_CONDUCTOR", LOG2DB * len * ac_factor * sr_er0 * sqrt((M_PI * MU0 * freq) / sigma));
    this.SetParameter("LOSS_DIELECTRIC", (LOG2DB * len * ad_factor * freq * (sr_er_f * sr_er_f - 1.0)) / sr_er_f);
    this.SetParameter("ANG_L", (2.0 * M_PI * len * sr_er_f * freq) / C0);
    this.SetParameter("EPSILON_EFF", sr_er_f * sr_er_f);
    this.SetParameter("Z0", zl_factor / sr_er_f);
    this.SetParameter("UNIT_PROP_DELAY", UnitPropagationDelay(sr_er_f * sr_er_f));
  }

  /** `COPLANAR::Synthesize`. */
  Synthesize(_aOpts: SynthesizeOpts): boolean {
    const target: TCP = this.m_synthesizeTarget === "PHYS_S" ? "PHYS_S" : "PHYS_WIDTH";
    return this.MinimiseZ0Error1D(target, "Z0", true);
  }

  private publish(set: (p: TCP, v: number, s?: TranslineStatus) => void, synthesis: boolean): void {
    for (const p of ["EPSILON_EFF", "UNIT_PROP_DELAY", "LOSS_CONDUCTOR", "LOSS_DIELECTRIC", "SKIN_DEPTH"] as const) set(p, this.GetParameter(p));
    const Z0 = this.GetParameter("Z0");
    const angL = this.GetParameter("ANG_L");
    const len = this.GetParameter("PHYS_LEN");
    const W = this.GetParameter("PHYS_WIDTH");
    const S = this.GetParameter("PHYS_S");
    const W_invalid = !isfinite(W) || W <= 0.0;
    const S_invalid = !isfinite(S) || S <= 0.0;
    set("Z0", Z0, st(!isfinite(Z0) || Z0 < 0, synthesis ? "WARNING" : "TS_ERROR"));
    set("ANG_L", angL, st(!isfinite(angL) || angL < 0, synthesis ? "WARNING" : "TS_ERROR"));
    set("PHYS_LEN", len, st(!isfinite(len) || len < 0, synthesis ? "TS_ERROR" : "WARNING"));
    set("PHYS_WIDTH", W, st(W_invalid, synthesis && this.m_synthesizeTarget === "PHYS_WIDTH" ? "TS_ERROR" : "WARNING"));
    set("PHYS_S", S, st(S_invalid, synthesis && this.m_synthesizeTarget === "PHYS_S" ? "TS_ERROR" : "WARNING"));
  }

  protected SetAnalysisResults(): void {
    this.publish((p, v, s) => this.SetAnalysisResult(p, v, s), false);
  }

  protected SetSynthesisResults(): void {
    this.publish((p, v, s) => this.SetSynthesisResult(p, v, s), true);
  }
}

// ------------------------------------------------------------------------------------------------------------------ RECTWAVEGUIDE

/**
 * rectwaveguide.{h,cpp} `RECTWAVEGUIDE`: a rectangular waveguide, width a (PHYS_WIDTH) by height b (PHYS_S), TE10 Z0 with the fictive voltage / current
 * definition and every propagating mode's conductor loss (Pozar section 3.3). Synthesis solves the width in closed form.
 */
export class RectWaveguide extends TranslineCalculationBase {
  private m_teModes = "";
  private m_tmModes = "";

  constructor() {
    super(["EPSILONR", "TAND", "SIGMA", "MUR", "MURC", "PHYS_WIDTH", "PHYS_S", "PHYS_LEN", "FREQUENCY", "Z0", "ANG_L", "LOSS_CONDUCTOR", "LOSS_DIELECTRIC", "CUTOFF_FREQUENCY", "SKIN_DEPTH", "EPSILON_EFF", "DIELECTRIC_MODEL_SEL", "EPSILONR_SPEC_FREQ"]);
    this.m_synthesizeTarget = "PHYS_WIDTH";
  }

  override SetSynthesizeTarget(aTarget: TCP): void {
    if (aTarget === "PHYS_WIDTH") this.m_synthesizeTarget = aTarget;
  }

  GetTEModes(): string {
    return this.m_teModes;
  }

  GetTMModes(): string {
    return this.m_tmModes;
  }

  private KvalSquare(): number {
    const kval = (2.0 * M_PI * this.GetParameter("FREQUENCY") * sqrt(this.GetParameter("MUR") * this.GetParameter("EPSILONR"))) / C0;
    return kval * kval;
  }

  private KcSquare(aM: number, aN: number): number {
    const a = this.GetParameter("PHYS_WIDTH");
    const b = this.GetParameter("PHYS_S");
    return pow((aM * M_PI) / a, 2.0) + pow((aN * M_PI) / b, 2.0);
  }

  private Fc(aM: number, aN: number): number {
    return (sqrt(this.KcSquare(aM, aN) / this.GetParameter("MUR") / this.GetParameter("EPSILONR")) * C0) / (2.0 * M_PI);
  }

  /** `AlphaC`: the conductor loss of every propagating TE_mn and TM_mn mode, dB/m. */
  private AlphaC(): number {
    const a = this.GetParameter("PHYS_WIDTH");
    const b = this.GetParameter("PHYS_S");
    const f = this.GetParameter("FREQUENCY");
    const murc = this.GetParameter("MURC");
    const sigma = this.GetParameter("SIGMA");
    const Rs = sqrt((M_PI * f * murc * MU0) / sigma);
    let ac = 0.0;
    const mmax = Math.trunc(floor(f / this.Fc(1, 0)));
    const nmax = mmax;
    for (let n = 0; n <= nmax; ++n) {
      for (let m = 1; m <= mmax; ++m) {
        const f_c = this.Fc(m, n);
        if (f <= f_c) continue;
        if (n === 0) {
          ac += (Rs / (b * ZF0 * sqrt(1.0 - pow(f_c / f, 2.0)))) * (1.0 + ((2.0 * b) / a) * pow(f_c / f, 2.0));
        } else {
          ac +=
            ((2.0 * Rs) / (b * ZF0 * sqrt(1.0 - pow(f_c / f, 2.0)))) *
            ((1.0 + b / a) * pow(f_c / f, 2.0) + (1.0 - pow(f_c / f, 2.0)) * (((b / a) * ((b / a) * pow(m, 2.0) + pow(n, 2.0))) / (pow((b * m) / a, 2.0) + pow(n, 2.0))));
        }
      }
    }
    for (let n = 1; n <= nmax; ++n) {
      for (let m = 1; m <= mmax; ++m) {
        const f_c = this.Fc(m, n);
        if (f <= f_c) continue;
        ac += ((2.0 * Rs) / (b * ZF0 * sqrt(1.0 - pow(f_c / f, 2.0)))) * ((pow(m, 2.0) * pow(b / a, 3.0) + pow(n, 2.0)) / (pow((m * b) / a, 2.0) + pow(n, 2.0)));
      }
    }
    return ac * LOG2DB;
  }

  private AlphaCCutoff(): number {
    let acc = sqrt(this.KcSquare(1, 0) - this.KvalSquare());
    acc = LOG2DB * acc;
    return acc;
  }

  private AlphaD(): number {
    const k_square = this.KvalSquare();
    const beta = sqrt(k_square - this.KcSquare(1, 0));
    let ad = (k_square * this.GetParameter("TAND")) / (2.0 * beta);
    ad = ad * LOG2DB;
    return ad;
  }

  /** `UpdateModeStrings`: the propagating modes up to index 6, cut to KiCad's 128-character buffer with "...". */
  private UpdateModeStrings(): void {
    const MAX_INDEX = 6;
    const MAX_STR = 128;
    const freq = this.GetParameter("FREQUENCY");
    this.m_teModes = "";
    this.m_tmModes = "";
    const walk = (first: number, letter: string): string => {
      let buf = "";
      let truncated = false;
      for (let m = first; m <= MAX_INDEX && !truncated; ++m) {
        for (let n = first; n <= MAX_INDEX && !truncated; ++n) {
          if (m === 0 && n === 0) continue;
          if (freq >= this.Fc(m, n)) {
            const token = `${letter}(${m},${n}) `;
            if (buf.length + token.length + 5 < MAX_STR) buf += token;
            else {
              buf += "...";
              truncated = true;
            }
          }
        }
      }
      return buf;
    };
    if (freq >= this.Fc(1, 0)) this.m_teModes = walk(0, "H");
    if (freq >= this.Fc(1, 1)) this.m_tmModes = walk(1, "E");
  }

  /** `RECTWAVEGUIDE::Analyse`. */
  Analyse(): void {
    this.UpdateDielectricModel();
    const freq = this.GetParameter("FREQUENCY");
    const rawEpsR = this.GetParameter("EPSILONR");
    const rawTanD = this.GetParameter("TAND");
    this.SetParameter("EPSILONR", this.GetDispersedEpsilonR(freq));
    this.SetParameter("TAND", this.GetDispersedTanDelta(freq));
    this.SetParameter("SKIN_DEPTH", this.SkinDepth());
    this.SetParameter("CUTOFF_FREQUENCY", this.Fc(1, 0));
    const len = this.GetParameter("PHYS_LEN");
    const k_square = this.KvalSquare();
    const kc10_square = this.KcSquare(1, 0);
    if (kc10_square <= k_square) {
      const fc10 = this.Fc(1, 0);
      this.SetParameter("Z0", (ZF0 * sqrt(this.GetParameter("MUR") / this.GetParameter("EPSILONR"))) / sqrt(1.0 - pow(fc10 / freq, 2.0)));
      const lambda_g = (2.0 * M_PI) / sqrt(k_square - kc10_square);
      this.SetParameter("ANG_L", (2.0 * M_PI * len) / lambda_g);
      this.SetParameter("LOSS_CONDUCTOR", this.AlphaC() * len);
      this.SetParameter("LOSS_DIELECTRIC", this.AlphaD() * len);
      this.SetParameter("EPSILON_EFF", 1.0 - pow(fc10 / freq, 2.0));
    } else {
      this.SetParameter("Z0", 0.0);
      this.SetParameter("ANG_L", 0.0);
      this.SetParameter("EPSILON_EFF", 0.0);
      this.SetParameter("LOSS_DIELECTRIC", 0.0);
      this.SetParameter("LOSS_CONDUCTOR", this.AlphaCCutoff() * len);
    }
    this.UpdateModeStrings();
    this.SetParameter("EPSILONR", rawEpsR);
    this.SetParameter("TAND", rawTanD);
  }

  /** `RECTWAVEGUIDE::Synthesize`: the closed-form inverse of TE10's Z0 for the width; true when the width is finite and positive. */
  Synthesize(_aOpts: SynthesizeOpts): boolean {
    this.UpdateDielectricModel();
    const mur = this.GetParameter("MUR");
    const freq = this.GetParameter("FREQUENCY");
    const Z0 = this.GetParameter("Z0");
    const angL = this.GetParameter("ANG_L");
    const rawEpsR = this.GetParameter("EPSILONR");
    const rawTanD = this.GetParameter("TAND");
    const epsr = this.GetDispersedEpsilonR(freq);
    this.SetParameter("EPSILONR", epsr);
    this.SetParameter("TAND", this.GetDispersedTanDelta(freq));
    const eta = ZF0 * sqrt(mur / epsr);
    this.SetParameter("PHYS_WIDTH", C0 / (sqrt(mur * epsr) * 2.0 * freq * sqrt(1.0 - pow(eta / Z0, 2.0))));
    const k_square = this.KvalSquare();
    const kc10_square = this.KcSquare(1, 0);
    const beta = sqrt(k_square - kc10_square);
    const lambda_g = (2.0 * M_PI) / beta;
    this.SetParameter("PHYS_LEN", (angL * lambda_g) / (2.0 * M_PI));
    if (kc10_square <= k_square) {
      const len = this.GetParameter("PHYS_LEN");
      this.SetParameter("LOSS_CONDUCTOR", this.AlphaC() * len);
      this.SetParameter("LOSS_DIELECTRIC", this.AlphaD() * len);
      this.SetParameter("EPSILON_EFF", 1.0 - pow(this.Fc(1, 0) / freq, 2.0));
    } else {
      this.SetParameter("EPSILON_EFF", 0.0);
      this.SetParameter("LOSS_DIELECTRIC", 0.0);
      this.SetParameter("LOSS_CONDUCTOR", this.AlphaCCutoff() * this.GetParameter("PHYS_LEN"));
    }
    this.SetParameter("SKIN_DEPTH", this.SkinDepth());
    this.SetParameter("CUTOFF_FREQUENCY", this.Fc(1, 0));
    this.UpdateModeStrings();
    this.SetParameter("Z0", Z0);
    this.SetParameter("ANG_L", angL);
    this.SetParameter("EPSILONR", rawEpsR);
    this.SetParameter("TAND", rawTanD);
    return isfinite(this.GetParameter("PHYS_WIDTH")) && this.GetParameter("PHYS_WIDTH") > 0.0;
  }

  private publish(set: (p: TCP, v: number, s?: TranslineStatus) => void, synthesis: boolean): void {
    for (const p of ["EPSILON_EFF", "LOSS_CONDUCTOR", "LOSS_DIELECTRIC", "CUTOFF_FREQUENCY", "SKIN_DEPTH"] as const) set(p, this.GetParameter(p));
    const Z0 = this.GetParameter("Z0");
    const angL = this.GetParameter("ANG_L");
    const len = this.GetParameter("PHYS_LEN");
    const a = this.GetParameter("PHYS_WIDTH");
    const b = this.GetParameter("PHYS_S");
    set("Z0", Z0, st(!isfinite(Z0) || Z0 < 0, synthesis ? "WARNING" : "TS_ERROR"));
    set("ANG_L", angL, st(!isfinite(angL) || angL < 0, synthesis ? "WARNING" : "TS_ERROR"));
    set("PHYS_LEN", len, st(!isfinite(len) || len < 0, synthesis ? "TS_ERROR" : "WARNING"));
    set("PHYS_WIDTH", a, st(!isfinite(a) || a <= 0.0, synthesis && this.m_synthesizeTarget === "PHYS_WIDTH" ? "TS_ERROR" : "WARNING"));
    set("PHYS_S", b, st(!isfinite(b) || b <= 0.0, "WARNING"));
  }

  protected SetAnalysisResults(): void {
    this.publish((p, v, s) => this.SetAnalysisResult(p, v, s), false);
  }

  protected SetSynthesisResults(): void {
    this.publish((p, v, s) => this.SetSynthesisResult(p, v, s), true);
  }
}

// ------------------------------------------------------------------------------------------------------------------ TWISTEDPAIR

/**
 * twistedpair.{h,cpp} `TWISTEDPAIR`: a twisted pair of insulated wires (Lefferson 1971; the twist angle in degrees), with Wadell's proximity factor in
 * the conductor loss and the insulation's filling factor in the dielectric loss. Synthesis solves the inner or the outer diameter (`SetSynthesizeTarget`,
 * the outer one by default) by `MinimiseZ0Error1D`, recomputing the length from the angular length.
 */
export class TwistedPair extends TranslineCalculationBase {
  constructor() {
    super(["EPSILONR", "TAND", "SIGMA", "MURC", "PHYS_DIAM_IN", "PHYS_DIAM_OUT", "PHYS_LEN", "FREQUENCY", "Z0", "ANG_L", "LOSS_CONDUCTOR", "LOSS_DIELECTRIC", "SKIN_DEPTH", "EPSILON_EFF", "UNIT_PROP_DELAY", "TWISTEDPAIR_TWIST", "TWISTEDPAIR_EPSILONR_ENV", "DIELECTRIC_MODEL_SEL", "EPSILONR_SPEC_FREQ"]);
    this.m_synthesizeTarget = "PHYS_DIAM_OUT";
  }

  override SetSynthesizeTarget(aTarget: TCP): void {
    if (aTarget === "PHYS_DIAM_IN" || aTarget === "PHYS_DIAM_OUT") this.m_synthesizeTarget = aTarget;
  }

  /** `TWISTEDPAIR::Analyse`. */
  Analyse(): void {
    this.UpdateDielectricModel();
    const twist = this.GetParameter("TWISTEDPAIR_TWIST");
    const epsrEnv = this.GetParameter("TWISTEDPAIR_EPSILONR_ENV");
    const Din = this.GetParameter("PHYS_DIAM_IN");
    const Dout = this.GetParameter("PHYS_DIAM_OUT");
    const freq = this.GetParameter("FREQUENCY");
    const len = this.GetParameter("PHYS_LEN");
    const epsr = this.GetDispersedEpsilonR(freq);
    const tand = this.GetDispersedTanDelta(freq);
    this.SetParameter("SKIN_DEPTH", this.SkinDepth());
    const theta_rad = atan(twist * M_PI * Dout);
    const theta_deg = theta_rad * (180.0 / M_PI);
    const beta = 0.25 + 0.0007 * theta_deg * theta_deg;
    const epsEff = epsrEnv + beta * (epsr - epsrEnv);
    this.SetParameter("EPSILON_EFF", epsEff);
    const z0 = (ZF0 / M_PI / sqrt(epsEff)) * acosh(Dout / Din);
    this.SetParameter("Z0", z0);
    const skinDepth = this.GetParameter("SKIN_DEPTH");
    const sigma = this.GetParameter("SIGMA");
    const currentDepth = Math.min(skinDepth, Din / 2.0);
    const spacing = Dout / Din;
    const proximity = spacing / sqrt(spacing * spacing - 1.0);
    const crowding = 1.0 + (proximity - 1.0) * (1.0 - (2.0 * currentDepth) / Din);
    this.SetParameter("LOSS_CONDUCTOR", (LOG2DB * len * crowding) / currentDepth / sigma / M_PI / z0 / (Din - currentDepth));
    const lossFraction = Math.min(1.0, (beta * epsr) / epsEff);
    this.SetParameter("LOSS_DIELECTRIC", ((LOG2DB * len * M_PI) / C0) * freq * sqrt(epsEff) * tand * lossFraction);
    this.SetParameter("ANG_L", (2.0 * M_PI * len * sqrt(epsEff) * freq) / C0);
    this.SetParameter("UNIT_PROP_DELAY", UnitPropagationDelay(epsEff));
  }

  /** `TWISTEDPAIR::Synthesize`. */
  Synthesize(_aOpts: SynthesizeOpts): boolean {
    const target: TCP = this.m_synthesizeTarget === "PHYS_DIAM_IN" ? "PHYS_DIAM_IN" : "PHYS_DIAM_OUT";
    return this.MinimiseZ0Error1D(target, "Z0", true);
  }

  private publish(set: (p: TCP, v: number, s?: TranslineStatus) => void, synthesis: boolean): void {
    for (const p of ["EPSILON_EFF", "LOSS_CONDUCTOR", "LOSS_DIELECTRIC", "SKIN_DEPTH"] as const) set(p, this.GetParameter(p));
    const Z0 = this.GetParameter("Z0");
    const angL = this.GetParameter("ANG_L");
    const len = this.GetParameter("PHYS_LEN");
    const Din = this.GetParameter("PHYS_DIAM_IN");
    const Dout = this.GetParameter("PHYS_DIAM_OUT");
    const Din_invalid = !isfinite(Din) || Din <= 0.0;
    const Dout_invalid = !isfinite(Dout) || Dout <= 0.0;
    const geometry_invalid = Din >= Dout;
    set("Z0", Z0, st(!isfinite(Z0) || Z0 <= 0, synthesis ? "WARNING" : "TS_ERROR"));
    set("ANG_L", angL, st(!isfinite(angL) || angL < 0, synthesis ? "WARNING" : "TS_ERROR"));
    set("PHYS_LEN", len, st(!isfinite(len) || len < 0, synthesis ? "TS_ERROR" : "WARNING"));
    if (!synthesis) {
      set("PHYS_DIAM_IN", Din, st(Din_invalid || geometry_invalid, "WARNING"));
      set("PHYS_DIAM_OUT", Dout, st(Dout_invalid || geometry_invalid, "WARNING"));
      return;
    }
    const tIn = this.m_synthesizeTarget === "PHYS_DIAM_IN";
    const tOut = this.m_synthesizeTarget === "PHYS_DIAM_OUT";
    set("PHYS_DIAM_IN", Din, geometry_invalid ? (tIn ? "TS_ERROR" : "WARNING") : st(Din_invalid, tIn ? "TS_ERROR" : "WARNING"));
    set("PHYS_DIAM_OUT", Dout, geometry_invalid ? (tOut ? "TS_ERROR" : "WARNING") : st(Dout_invalid, tOut ? "TS_ERROR" : "WARNING"));
  }

  protected SetAnalysisResults(): void {
    this.publish((p, v, s) => this.SetAnalysisResult(p, v, s), false);
  }

  protected SetSynthesisResults(): void {
    this.publish((p, v, s) => this.SetSynthesisResult(p, v, s), true);
  }
}
