// The RF Attenuators panel's calculators (pcb_calculator/attenuators/attenuator_classes.{h,cpp}, KiCad 25ec2335): Pi, Tee, Bridged tee and the
// resistive splitter, each turning an attenuation (dB) and the source / load impedances into R1, R2 (and R3). `ATTENUATOR::Calculate` also finds the
// smallest attenuation a mismatch of Zin and Zout allows (`m_MinimumATT`) and sets `m_Error` when the attenuation asked is below it. The arithmetic is
// the C++'s, in its order.
//
// Pure: no React, no DOM.
import { BRIDGED_TEE_FORMULA, PI_FORMULA, SPLITTER_FORMULA, TEE_FORMULA } from "./formulaHelp";
import type { AttenuatorName } from "./calcSettings";

/** `ATTENUATORS_TYPE`. */
export type AttenuatorTopology = "PI_TYPE" | "TEE_TYPE" | "BRIDGE_TYPE" | "SPLITTER_TYPE";

/** `ATTENUATOR`: the base class's fields and its Calculate (Lmin, L, A are its temporaries). */
export class Attenuator {
  /** Identifier for configuration (`m_Name`). */
  name: AttenuatorName = "att_pi";
  /** Number of values to calculate, and therefore display. */
  resultCount = 3;
  /** Set when the values cannot be calculated. */
  error = false;
  zin = 50;
  zinEnable = true;
  zout = 50;
  attenuation = 6.0;
  attenuationEnable = true;
  minimumAtt = 0.0;
  r1 = 0.0;
  r2 = 0.0;
  r3 = 0.0;
  /** The markdown of its formula (`m_FormulaName`). */
  formula: string | null = null;
  readonly topology: AttenuatorTopology;
  protected Lmin = 0.0;
  protected L = 0.0;
  protected A = 0.0;

  constructor(topology: AttenuatorTopology) {
    this.topology = topology;
  }

  /** `ATTENUATOR::Calculate`. */
  calculate(): boolean {
    this.L = Math.pow(10, this.attenuation / 10);
    this.A = (this.L + 1) / (this.L - 1);
    if (this.zin > this.zout) this.Lmin = (2 * this.zin) / this.zout - 1 + 2 * Math.sqrt((this.zin / this.zout) * (this.zin / this.zout - 1));
    else this.Lmin = (2 * this.zout) / this.zin - 1 + 2 * Math.sqrt((this.zout / this.zin) * (this.zout / this.zin - 1));
    this.minimumAtt = 10 * Math.log10(this.Lmin);
    if (this.minimumAtt > this.attenuation) {
      this.error = true;
      return false;
    }
    this.error = false;
    return true;
  }
}

/** `ATTENUATOR_PI`. */
export class AttenuatorPi extends Attenuator {
  constructor() {
    super("PI_TYPE");
    this.name = "att_pi";
    this.formula = PI_FORMULA;
  }

  override calculate(): boolean {
    if (!super.calculate()) return false;
    this.r2 = ((this.L - 1) / 2) * Math.sqrt((this.zin * this.zout) / this.L);
    this.r1 = 1 / (this.A / this.zin - 1 / this.r2);
    this.r3 = 1 / (this.A / this.zout - 1 / this.r2);
    return true;
  }
}

/** `ATTENUATOR_TEE`. */
export class AttenuatorTee extends Attenuator {
  constructor() {
    super("TEE_TYPE");
    this.name = "att_tee";
    this.formula = TEE_FORMULA;
  }

  override calculate(): boolean {
    if (!super.calculate()) return false;
    this.r2 = (2 * Math.sqrt(this.L * this.zin * this.zout)) / (this.L - 1);
    this.r1 = this.zin * this.A - this.r2;
    this.r3 = this.zout * this.A - this.r2;
    return true;
  }
}

/** `ATTENUATOR_BRIDGE`: Zin is Zout; two results. */
export class AttenuatorBridge extends Attenuator {
  constructor() {
    super("BRIDGE_TYPE");
    this.name = "att_bridge";
    this.zinEnable = false;
    this.resultCount = 2;
    this.formula = BRIDGED_TEE_FORMULA;
  }

  override calculate(): boolean {
    this.zin = this.zout;
    if (!super.calculate()) return false;
    this.L = Math.pow(10, this.attenuation / 20);
    this.r1 = this.zin * (this.L - 1);
    this.r2 = this.zin / (this.L - 1);
    return true;
  }
}

/** `ATTENUATOR_SPLITTER`: always 6 dB, Zin is Zout, three equal resistors. */
export class AttenuatorSplitter extends Attenuator {
  constructor() {
    super("SPLITTER_TYPE");
    this.name = "att_splitter";
    this.attenuationEnable = false;
    this.attenuation = 6.0;
    this.minimumAtt = 6.0;
    this.zinEnable = false;
    this.formula = SPLITTER_FORMULA;
  }

  override calculate(): boolean {
    this.attenuation = 6.0;
    this.zin = this.zout;
    this.r1 = this.r2 = this.r3 = this.zout / 3.0;
    return true;
  }
}

/** The panel's list, "ordered like in dialog menu list" (Pi, Tee, Bridged tee, Resistive splitter). */
export function makeAttenuatorList(): Attenuator[] {
  return [new AttenuatorPi(), new AttenuatorTee(), new AttenuatorBridge(), new AttenuatorSplitter()];
}
