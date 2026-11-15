// IEC 60664-1 : 2020-05 clearances and creepage distances (pcb_calculator/calculator_panels/iec60664.{h,cpp} at KiCad 25ec2335), the Electrical Spacing
// calculator's "IEC 60664" page (components/calculator/PanelElectricalSpacing.tsx).
//
// The C++ class translated statement for statement (the long chains of `if( aVoltage <= x ) return y;` are KiCad's tables A.1, A.2, F.1, F.2, F.5 and
// F.8 as it has them, duplicated rows and all); the enums keep their C++ values, so the `aPD <= POLLUTION_DEGREE::PD2` comparisons read the same.
// The one rename: the private `GetMinGrooveWidth( aPD, aDistIso )` overload is `GetMinGrooveWidthFor` (TypeScript has no overloads by arity).
// iec60664.test.ts holds this port to the output of KiCad's own iec60664.cpp compiled unchanged (iec60664.golden.json, tools/iec60664-golden/).
//
// Pure: no React, no DOM.

export const POLLUTION_DEGREE = { PD1: 1, PD2: 2, PD3: 3, PD4: 4 } as const;
export const OV_CATEGORY = { OV_I: 1, OV_II: 2, OV_III: 3, OV_IV: 4 } as const;
export const MATERIAL_GROUP = { MG_I: 1, MG_II: 2, MG_IIIa: 3, MG_IIIb: 4, NOT_INSULATING: 5 } as const;
export const INSULATION_TYPE = { FUNCTIONAL: 1, BASIC: 2, REINFORCED: 3 } as const;
export const FIELD = { INHOMOGENEOUS: 1, HOMOGENEOUS: 2 } as const;
export const WAVEFORM = { AC_50_OR_60HZ: 1, IMPULSE_1_2_50: 2 } as const;

export type PollutionDegree = (typeof POLLUTION_DEGREE)[keyof typeof POLLUTION_DEGREE];
export type OvCategory = (typeof OV_CATEGORY)[keyof typeof OV_CATEGORY];
export type MaterialGroup = (typeof MATERIAL_GROUP)[keyof typeof MATERIAL_GROUP];
export type InsulationType = (typeof INSULATION_TYPE)[keyof typeof INSULATION_TYPE];
export type Field = (typeof FIELD)[keyof typeof FIELD];

export class IEC60664 {
  m_ratedVoltage = 100e3;
  m_frequency = 50;
  m_coated = false;
  m_pcbMaterial = false;
  m_RMSvoltage = 100e3;
  m_peakVoltage = 100e3;
  m_transientVoltage = 100e3;
  m_altitude = 100e3;
  m_clearance = -1;
  m_creepage = -1;
  m_minGrooveWidth = -1;
  m_field: Field = FIELD.INHOMOGENEOUS;
  m_overvoltageCat: OvCategory = OV_CATEGORY.OV_IV;
  m_materialGroup: MaterialGroup = MATERIAL_GROUP.NOT_INSULATING;
  m_pollutionDegree: PollutionDegree = POLLUTION_DEGREE.PD3;
  m_insulationType: InsulationType = INSULATION_TYPE.REINFORCED;

  SetAltitude(aAltitude: number): void { this.m_altitude = aAltitude; }
  SetPollutionDegree(aPD: PollutionDegree): void { this.m_pollutionDegree = aPD; }
  SetPCBMaterial(aPCB: boolean): void { this.m_pcbMaterial = aPCB; }
  SetMaterialGroup(aMG: MaterialGroup): void { this.m_materialGroup = aMG; }
  SetFrequency(aF: number): void { this.m_frequency = aF; }
  SetInsulationType(aIns: InsulationType): void { this.m_insulationType = aIns; }
  SetOvervoltageCategory(aOV: OvCategory): void { this.m_overvoltageCat = aOV; }
  SetField(aField: Field): void { this.m_field = aField; }
  SetRatedVoltage(aV: number): void { this.m_ratedVoltage = aV; }
  SetRMSVoltage(aV: number): void { this.m_RMSvoltage = aV; }
  SetPeakVoltage(aV: number): void { this.m_peakVoltage = aV; }
  SetTransientVoltage(aV: number): void { this.m_transientVoltage = aV; }

  GetMinGrooveWidth(): number { return this.m_minGrooveWidth; }
  GetClearanceDistance(): number { return this.m_clearance; }
  GetCreepageDistance(): number { return this.m_creepage; }

  /** `IEC60664::GetMinGrooveWidth( aPD, aDistIso )` (private; the public one is the getter `GetMinGrooveWidth()`). */
  GetMinGrooveWidthFor(aPD: number, aDistIso: number): number {
      if( aDistIso <= 0 )
          return -1;
  
      //  Based on IEC60664-1 : 2020-05 §6.8
      if( Math.abs( aDistIso ) < 3 )
          return aDistIso / 3;
  
      switch( aPD )
      {
      case POLLUTION_DEGREE.PD1: return 0.25;
      case POLLUTION_DEGREE.PD2: return 1.0;
      case POLLUTION_DEGREE.PD3: return 1.5;
      default: return -1;
      }
  }

  /** `IEC60664::GetClearanceAltitudeCorrectionFactor`. */
  GetClearanceAltitudeCorrectionFactor(aAltitude: number): number {
      //  Based on IEC60664-1 : 2020-05 Table A.2
  
      if( aAltitude <= 2000 )
          return 1.0;
      if( aAltitude <= 3000 )
          return 1.14;
      if( aAltitude <= 4000 )
          return 1.29;
      if( aAltitude <= 5000 )
          return 1.48;
      if( aAltitude <= 6000 )
          return 1.70;
      if( aAltitude <= 7000 )
          return 1.95;
      if( aAltitude <= 8000 )
          return 2.25;
      if( aAltitude <= 9000 )
          return 2.62;
      if( aAltitude <= 10000 )
          return 3.02;
      if( aAltitude <= 15000 )
          return 6.67;
      if( aAltitude <= 20000 )
          return 14.5;
      return -1;
  }

  /** `IEC60664::GetClearanceForRMSVoltage`. */
  GetClearanceForRMSVoltage(aVRMS: number, aField: number): number {
      //  Based on IEC60664-1 : 2020-05 Table A.1
  
      switch( aField )
      {
      case FIELD.INHOMOGENEOUS:
           if( aVRMS <= 0.028 )
              return 0.001;
          if( aVRMS <= 0.053 )
              return 0.002;
          if( aVRMS <= 0.078 )
              return 0.003;
          if( aVRMS <= 0.102 )
              return 0.004;
          if( aVRMS <= 0.124 )
              return 0.005;
          if( aVRMS <= 0.152 )
              return 0.00625;
          if( aVRMS <= 0.191 )
              return 0.008;
          if( aVRMS <= 0.23 )
              return 0.010;
          if( aVRMS <= 0.25 )
              return 0.012;
          if( aVRMS <= 0.26 )
              return 0.015;
          if( aVRMS <= 0.28 )
              return 0.020;
          if( aVRMS <= 0.31 )
              return 0.025;
          if( aVRMS <= 0.33 )
              return 0.030;
          if( aVRMS <= 0.37 )
              return 0.040;
          if( aVRMS <= 0.37 )
              return 0.040;
          if( aVRMS <= 0.40 )
              return 0.050;
          if( aVRMS <= 0.42 )
              return 0.0625;
          if( aVRMS <= 0.46 )
              return 0.080;
          if( aVRMS <= 0.50 )
              return 0.10;
          if( aVRMS <= 0.52 )
              return 0.12;
          if( aVRMS <= 0.57 )
              return 0.15;
          if( aVRMS <= 0.62 )
              return 0.20;
          if( aVRMS <= 0.67 )
              return 0.25;
          if( aVRMS <= 0.71 )
              return 0.30;
          if( aVRMS <= 0.78 )
              return 0.40;
          if( aVRMS <= 0.84 )
              return 0.50;
          if( aVRMS <= 0.90 )
              return 0.60;
          if( aVRMS <= 0.98 )
              return 0.80;
          if( aVRMS <= 1.06 )
              return 1.0;
          if( aVRMS <= 1.20 )
              return 1.2;
          if( aVRMS <= 1.39 )
              return 1.5;
          if( aVRMS <= 1.68 )
              return 2.0;
          if( aVRMS <= 1.96 )
              return 2.5;
          if( aVRMS <= 2.21 )
              return 3.0;
          if( aVRMS <= 2.68 )
              return 4.0;
          if( aVRMS <= 3.11 )
              return 5.0;
          if( aVRMS <= 3.51 )
              return 6.0;
          if( aVRMS <= 4.26 )
              return 8.0;
          if( aVRMS <= 4.95 )
              return 10;
          if( aVRMS <= 5.78 )
              return 12.0;
          if( aVRMS <= 7.0 )
              return 15.0;
          if( aVRMS <= 8.98 )
              return 20.0;
          if( aVRMS <= 10.8 )
              return 25.0;
          if( aVRMS <= 12.7 )
              return 30.0;
          if( aVRMS <= 16.2 )
              return 40.0;
          if( aVRMS <= 19.6 )
              return 50.0;
          if( aVRMS <= 22.8 )
              return 60.0;
          if( aVRMS <= 29.2 )
              return 80.0;
          if( aVRMS <= 35.4 )
              return 80.0;
  
          break;
  
      case FIELD.HOMOGENEOUS:
          if( aVRMS <= 0.028 )
              return 0.001;
          if( aVRMS <= 0.053 )
              return 0.002;
          if( aVRMS <= 0.078 )
              return 0.003;
          if( aVRMS <= 0.102 )
              return 0.004;
          if( aVRMS <= 0.124 )
              return 0.005;
          if( aVRMS <= 0.152 )
              return 0.00625;
          if( aVRMS <= 0.191 )
              return 0.008;
          if( aVRMS <= 0.23 )
              return 0.010;
          if( aVRMS <= 0.25 )
              return 0.012;
          if( aVRMS <= 0.26 )
              return 0.015;
          if( aVRMS <= 0.28 )
              return 0.020;
          if( aVRMS <= 0.31 )
              return 0.025;
          if( aVRMS <= 0.33 )
              return 0.030;
          if( aVRMS <= 0.37 )
              return 0.040;
          if( aVRMS <= 0.37 )
              return 0.040;
          if( aVRMS <= 0.40 )
              return 0.050;
          if( aVRMS <= 0.42 )
              return 0.0625;
          if( aVRMS <= 0.50 )
              return 0.080;
          if( aVRMS <= 0.57 )
              return 0.10;
          if( aVRMS <= 0.64 )
              return 0.12;
          if( aVRMS <= 0.74 )
              return 0.15;
          if( aVRMS <= 0.89 )
              return 0.20;
          if( aVRMS <= 1.03 )
              return 0.25;
          if( aVRMS <= 1.15 )
              return 0.30;
          if( aVRMS <= 1.38 )
              return 0.40;
          if( aVRMS <= 1.59 )
              return 0.50;
          if( aVRMS <= 1.79 )
              return 0.60;
          if( aVRMS <= 2.15 )
              return 0.80;
          if( aVRMS <= 2.47 )
              return 1.0;
          if( aVRMS <= 2.89 )
              return 1.2;
          if( aVRMS <= 3.50 )
              return 1.5;
          if( aVRMS <= 4.48 )
              return 2.0;
          if( aVRMS <= 5.41 )
              return 2.5;
          if( aVRMS <= 6.32 )
              return 3.0;
          if( aVRMS <= 8.06 )
              return 4.0;
          if( aVRMS <= 9.76 )
              return 5.0;
          if( aVRMS <= 11.5 )
              return 6.0;
          if( aVRMS <= 14.6 )
              return 8.0;
          if( aVRMS <= 17.7 )
              return 10;
          if( aVRMS <= 20.9 )
              return 12.0;
          if( aVRMS <= 25.7 )
              return 15.0;
          if( aVRMS <= 33.5 )
              return 20.0;
          if( aVRMS <= 41.2 )
              return 25.0;
          if( aVRMS <= 48.8 )
              return 30.0;
          if( aVRMS <= 63.6 )
              return 40.0;
          if( aVRMS <= 78.5 )
              return 50.0;
          if( aVRMS <= 92.6 )
              return 60.0;
          if( aVRMS <= 120.9 )
              return 80.0;
          if( aVRMS <= 148.5 )
              return 80.0;
  
          break;
  
      default:
          break;
      }
  
      return -1;      // Out of range
  }

  /** `IEC60664::GetClearanceToWithstandTransientVoltage`. */
  GetClearanceToWithstandTransientVoltage(aVoltage: number, aPD: number, aField: number): number {
      //  Based on IEC60664-1 : 2020-05 Table F.2
  
      switch( aField )
      {
      case FIELD.INHOMOGENEOUS:
          if( aPD <= POLLUTION_DEGREE.PD1 )
          {
              if( aVoltage <= 0.33 )
                  return 0.01;
              if( aVoltage <= 0.40 )
                  return 0.02;
              if( aVoltage <= 0.50 )
                  return 0.04;
              if( aVoltage <= 0.60 )
                  return 0.06;
              if( aVoltage <= 0.80 )
                  return 0.10;
              if( aVoltage <= 1.0 )
                  return 0.15;
          }
          if( aPD <= POLLUTION_DEGREE.PD2 )
          {
              if( aVoltage <= 1.0 )
                  return 0.2;
              if( aVoltage <= 1.2 )
                  return 0.25;
              if( aVoltage <= 1.5 )
                  return 0.5;
          }
          if( aPD <= POLLUTION_DEGREE.PD3 )
          {
              if( aVoltage <= 1.5 )
                  return 0.8;
              if( aVoltage <= 2.0 )
                  return 1.0;
              if( aVoltage <= 2.5 )
                  return 1.5;
          }
          if( ( aPD >= POLLUTION_DEGREE.PD4 ) && ( aVoltage <= 2.5 ) )
              return 1.6;
          if( aVoltage <= 3.0 )
              return 2.0;
          if( aVoltage <= 4.0 )
              return 3.0;
          if( aVoltage <= 5.0 )
              return 4.0;
          if( aVoltage <= 6.0 )
              return 5.5;
          if( aVoltage <= 8.0 )
              return 8.0;
          if( aVoltage <= 10 )
              return 11;
          if( aVoltage <= 12 )
              return 14;
          if( aVoltage <= 15 )
              return 18;
          if( aVoltage <= 20 )
              return 25;
          if( aVoltage <= 25 )
              return 33;
          if( aVoltage <= 30 )
              return 40;
          if( aVoltage <= 40 )
              return 60;
          if( aVoltage <= 50 )
              return 75;
          if( aVoltage <= 60 )
              return 90;
          if( aVoltage <= 80 )
              return 130;
          if( aVoltage <= 100 )
              return 170;
  
          break;
  
      case FIELD.HOMOGENEOUS:
          if( aPD <= POLLUTION_DEGREE.PD1 )
          {
              if( aVoltage <= 0.33 )
                  return 0.01;
              if( aVoltage <= 0.40 )
                  return 0.02;
              if( aVoltage <= 0.50 )
                  return 0.04;
              if( aVoltage <= 0.60 )
                  return 0.06;
              if( aVoltage <= 0.80 )
                  return 0.10;
              if( aVoltage <= 1.0 )
                  return 0.15;
              if( aVoltage <= 1.2 )
                  return 0.2;
          }
          if( aPD <= POLLUTION_DEGREE.PD2 )
          {
              if( aVoltage <= 1.2 )
                  return 0.2;
              if( aVoltage <= 1.5 )
                  return 0.3;
              if( aVoltage <= 2.0 )
                  return 0.45;
              if( aVoltage <= 2.5 )
                  return 0.60;
              if( aVoltage <= 3.0 )
                  return 0.80;
          }
          if( aPD <= POLLUTION_DEGREE.PD3 )
          {
              if( aVoltage <= 3.0 )
                  return 0.80;
              if( aVoltage <= 4.0 )
                  return 1.2;
              if( aVoltage <= 5.0 )
                  return 1.5;
          }
          if( ( aPD >= POLLUTION_DEGREE.PD4 ) && ( aVoltage <= 5.0 ) )
              return 1.6;
          if( aVoltage <= 6.0 )
              return 2.0;
          if( aVoltage <= 8.0 )
              return 3.0;
          if( aVoltage <= 10 )
              return 3.5;
          if( aVoltage <= 12 )
              return 4.5;
          if( aVoltage <= 15 )
              return 5.5;
          if( aVoltage <= 20 )
              return 8.0;
          if( aVoltage <= 25 )
              return 10;
          if( aVoltage <= 30 )
              return 12.5;
          if( aVoltage <= 40 )
              return 17;
          if( aVoltage <= 50 )
              return 22;
          if( aVoltage <= 60 )
              return 27;
          if( aVoltage <= 80 )
              return 35;
          if( aVoltage <= 100 )
              return 45;
  
          break;
  
      default:
          break;
      }
  
      return -1;  // Out of range
  }

  /** `IEC60664::GetClearanceToWithstandPeaks`. */
  GetClearanceToWithstandPeaks(aVoltage: number, aField: number): number {
      //  Based on IEC60664-1 : 2020-05 Table F.8
  
      switch( aField )
      {
      case FIELD.INHOMOGENEOUS:
          if( aVoltage <= 0.04 )
              return 0.001;
          if( aVoltage <= 0.06 )
              return 0.002;
          if( aVoltage <= 0.1 )
              return 0.003;
          if( aVoltage <= 0.12 )
              return 0.004;
          if( aVoltage <= 0.15 )
              return 0.005;
          if( aVoltage <= 0.20 )
              return 0.006;
          if( aVoltage <= 0.25 )
              return 0.008;
          if( aVoltage <= 0.33 )
              return 0.01;
          if( aVoltage <= 0.4 )
              return 0.02;
          if( aVoltage <= 0.5 )
              return 0.04;
          if( aVoltage <= 0.6 )
              return 0.06;
          if( aVoltage <= 0.8 )
              return 0.13;
          if( aVoltage <= 1.0 )
              return 0.26;
          if( aVoltage <= 1.2 )
              return 0.42;
          if( aVoltage <= 1.5 )
              return 0.76;
          if( aVoltage <= 2.0 )
              return 1.27;
          if( aVoltage <= 2.5 )
              return 1.8;
          if( aVoltage <= 3.0 )
              return 2.4;
          if( aVoltage <= 4.0 )
              return 3.8;
          if( aVoltage <= 5.0 )
              return 5.7;
          if( aVoltage <= 6.0 )
              return 7.9;
          if( aVoltage <= 8.0 )
              return 11.0;
          if( aVoltage <= 10 )
              return 15.2;
          if( aVoltage <= 12 )
              return 19;
          if( aVoltage <= 15 )
              return 25;
          if( aVoltage <= 20 )
              return 34;
          if( aVoltage <= 25 )
              return 44;
          if( aVoltage <= 30 )
              return 55;
          if( aVoltage <= 40 )
              return 77;
          if( aVoltage <= 50 )
              return 100;
  
          break;
  
      case FIELD.HOMOGENEOUS:
          if( aVoltage <= 0.04 )
              return 0.001;
          if( aVoltage <= 0.06 )
              return 0.002;
          if( aVoltage <= 0.1 )
              return 0.003;
          if( aVoltage <= 0.12 )
              return 0.004;
          if( aVoltage <= 0.15 )
              return 0.005;
          if( aVoltage <= 0.20 )
              return 0.006;
          if( aVoltage <= 0.25 )
              return 0.008;
          if( aVoltage <= 0.33 )
              return 0.01;
          if( aVoltage <= 0.33 )
              return 0.01;
          if( aVoltage <= 0.4 )
              return 0.02;
          if( aVoltage <= 0.5 )
              return 0.04;
          if( aVoltage <= 0.6 )
              return 0.06;
          if( aVoltage <= 0.8 )
              return 0.1;
          if( aVoltage <= 1.0 )
              return 0.15;
          if( aVoltage <= 1.2 )
              return 0.2;
          if( aVoltage <= 1.5 )
              return 0.3;
          if( aVoltage <= 2.0 )
              return 0.45;
          if( aVoltage <= 2.5 )
              return 0.6;
          if( aVoltage <= 3.0 )
              return 0.8;
          if( aVoltage <= 4.0 )
              return 1.2;
          if( aVoltage <= 5.0 )
              return 1.5;
          if( aVoltage <= 6.0 )
              return 2;
          if( aVoltage <= 8.0 )
              return 3;
          if( aVoltage <= 10 )
              return 3.5;
          if( aVoltage <= 12 )
              return 4.5;
          if( aVoltage <= 15 )
              return 5.5;
          if( aVoltage <= 20 )
              return 8;
          if( aVoltage <= 25 )
              return 10;
          if( aVoltage <= 30 )
              return 12.5;
          if( aVoltage <= 40 )
              return 17;
          if( aVoltage <= 50 )
              return 22;
          if( aVoltage <= 60 )
              return 27;
          if( aVoltage <= 80 )
              return 35;
          if( aVoltage <= 100 )
              return 45;
  
          break;
  
      default:
          break;
      }
      return -1;
  }

  /** `IEC60664::GetRatedImpulseWithstandVoltage`. */
  GetRatedImpulseWithstandVoltage(): number {
      //  Based on IEC60664-1 : 2020-05 Table F.1
      let voltage = this.m_ratedVoltage;
  
      switch( this.m_overvoltageCat )
      {
      case OV_CATEGORY.OV_I:
          if( voltage <= 50 )
              return 330;
          if( voltage <= 100 )
              return 500;
          if( voltage <= 150 )
              return 800;
          if( voltage <= 300 )
              return 1500;
          if( voltage <= 600 )
              return 2500;
          if( voltage <= 1000 )
              return 4000;
          if( voltage <= 1250 )
              return 4000;
          if( voltage <= 1500 )
              return 6000;
  
          break;
  
      case OV_CATEGORY.OV_II:
          if( voltage <= 50 )
              return 500;
          if( voltage <= 100 )
              return 800;
          if( voltage <= 150 )
              return 1500;
          if( voltage <= 300 )
              return 2500;
          if( voltage <= 600 )
              return 4000;
          if( voltage <= 1000 )
              return 6000;
          if( voltage <= 1250 )
              return 6000;
          if( voltage <= 1500 )
              return 8000;
  
          break;
  
      case OV_CATEGORY.OV_III:
           if( voltage <= 50 )
              return 800;
          if( voltage <= 100 )
              return 1500;
          if( voltage <= 150 )
              return 2500;
          if( voltage <= 300 )
              return 4000;
          if( voltage <= 600 )
              return 6000;
          if( voltage <= 1000 )
              return 8000;
          if( voltage <= 1250 )
              return 8000;
          if( voltage <= 1500 )
              return 10000;
  
          break;
  
      case OV_CATEGORY.OV_IV:
          if( voltage <= 50 )
              return 1500;
          if( voltage <= 100 )
              return 2500;
          if( voltage <= 150 )
              return 4000;
          if( voltage <= 300 )
              return 6000;
          if( voltage <= 600 )
              return 8000;
          if( voltage <= 1000 )
              return 12000;
          if( voltage <= 1250 )
              return 12000;
          if( voltage <= 1500 )
              return 15000;
  
          break;
  
      default:
          break;
      }
  
      return -1;      // Out of range
  }

  /** `IEC60664::GetMaterialGroupFromCTI`. */
  GetMaterialGroupFromCTI(aCTI: number): MaterialGroup {
      if( aCTI >= 600 )
          return MATERIAL_GROUP.MG_I;
      if( aCTI >= 400 )
          return MATERIAL_GROUP.MG_II;
      if( aCTI >= 175 )
          return MATERIAL_GROUP.MG_IIIa;
      if( aCTI >= 100 )
          return MATERIAL_GROUP.MG_IIIb;
      return MATERIAL_GROUP.NOT_INSULATING;
  }

  /** `IEC60664::GetBasicCreepageDistance`. */
  GetBasicCreepageDistance(aVoltage: number, aPD: number, aMG: number): number {
      //  Based on IEC60664-1 : 2020-05 Table F.5
  
      let IsPCBmaterial = this.m_pcbMaterial;
  
      if( aVoltage > 1000 )
          IsPCBmaterial = false;
      if( aPD >= POLLUTION_DEGREE.PD3 )
          IsPCBmaterial = false;
      if( aPD >= POLLUTION_DEGREE.PD2 && aMG == MATERIAL_GROUP.MG_IIIb )
          IsPCBmaterial = false;
  
      if( IsPCBmaterial )
      {
          if( aPD == POLLUTION_DEGREE.PD1 )
          {
              if( aVoltage <= 50 )
                  return 0.025;
              if( aVoltage <= 63 )
                  return 0.040;
              if( aVoltage <= 80 )
                  return 0.063;
              if( aVoltage <= 100 )
                  return 0.100;
              if( aVoltage <= 125 )
                  return 0.160;
              if( aVoltage <= 160 )
                  return 0.250;
              if( aVoltage <= 200 )
                  return 0.400;
              if( aVoltage <= 250 )
                  return 0.560;
              if( aVoltage <= 320 )
                  return 0.75;
              if( aVoltage <= 400 )
                  return 1.0;
              if( aVoltage <= 500 )
                  return 1.3;
              if( aVoltage <= 630 )
                  return 1.8;
              if( aVoltage <= 800 )
                  return 2.4;
              if( aVoltage <= 1000 )
                  return 3.2;
          }
          if( aPD == POLLUTION_DEGREE.PD2 )
          {
              if( aVoltage <= 50 )
                  return 0.040;
              if( aVoltage <= 63 )
                  return 0.063;
              if( aVoltage <= 80 )
                  return 0.100;
              if( aVoltage <= 100 )
                  return 0.160;
              if( aVoltage <= 125 )
                  return 0.250;
              if( aVoltage <= 160 )
                  return 0.400;
              if( aVoltage <= 200 )
                  return 0.630;
              if( aVoltage <= 250 )
                  return 1.000;
              if( aVoltage <= 320 )
                  return 1.60;
              if( aVoltage <= 400 )
                  return 2.0;
              if( aVoltage <= 500 )
                  return 2.5;
              if( aVoltage <= 630 )
                  return 3.2;
              if( aVoltage <= 800 )
                  return 4.0;
              if( aVoltage <= 1000 )
                  return 5.0;
          }
      }
      if( aPD == POLLUTION_DEGREE.PD1 )
      {
          if( aVoltage <= 10 )
              return 0.080;
          if( aVoltage <= 12.5 )
              return 0.090;
          if( aVoltage <= 16 )
              return 0.100;
          if( aVoltage <= 20 )
              return 0.110;
          if( aVoltage <= 25 )
              return 0.125;
          if( aVoltage <= 32 )
              return 0.14;
          if( aVoltage <= 40 )
              return 0.16;
          if( aVoltage <= 50 )
              return 0.18;
          if( aVoltage <= 63 )
              return 0.20;
          if( aVoltage <= 80 )
              return 0.22;
          if( aVoltage <= 100 )
              return 0.25;
          if( aVoltage <= 125 )
              return 0.28;
          if( aVoltage <= 160 )
              return 0.32;
          if( aVoltage <= 200 )
              return 0.42;
          if( aVoltage <= 250 )
              return 0.56;
          if( aVoltage <= 320 )
              return 0.75;
          if( aVoltage <= 400 )
              return 1.0;
          if( aVoltage <= 500 )
              return 1.3;
          if( aVoltage <= 630 )
              return 1.8;
          if( aVoltage <= 800 )
              return 2.4;
          if( aVoltage <= 1000 )
              return 3.2;
          if( aVoltage <= 1250 )
              return 4.2;
          if( aVoltage <= 1600 )
              return 5.6;
          if( aVoltage <= 2000 )
              return 7.5;
          if( aVoltage <= 2500 )
              return 10.0;
          if( aVoltage <= 3200 )
              return 12.5;
          if( aVoltage <= 4000 )
              return 16.0;
          if( aVoltage <= 5000 )
              return 20.0;
          if( aVoltage <= 6300 )
              return 25.0;
          if( aVoltage <= 8000 )
              return 32.0;
          if( aVoltage <= 10000 )
              return 40.0;
          if( aVoltage <= 12500 )
              return 50.0;
          if( aVoltage <= 16000 )
              return 63.0;
          if( aVoltage <= 20000 )
              return 80.0;
          if( aVoltage <= 25000 )
              return 100.0;
          if( aVoltage <= 32000 )
              return 125.0;
          if( aVoltage <= 40000 )
              return 160.0;
          if( aVoltage <= 50000 )
              return 200.0;
          if( aVoltage <= 63000 )
              return 250;
      }
      if( aPD == POLLUTION_DEGREE.PD2 && aMG == MATERIAL_GROUP.MG_I )
      {
          if( aVoltage <= 10 )
              return 0.400;
          if( aVoltage <= 12.5 )
              return 0.420;
          if( aVoltage <= 16 )
              return 0.450;
          if( aVoltage <= 20 )
              return 0.480;
          if( aVoltage <= 25 )
              return 0.500;
          if( aVoltage <= 32 )
              return 0.53;
          if( aVoltage <= 40 )
              return 0.56;
          if( aVoltage <= 50 )
              return 0.60;
          if( aVoltage <= 63 )
              return 0.63;
          if( aVoltage <= 80 )
              return 0.67;
          if( aVoltage <= 100 )
              return 0.71;
          if( aVoltage <= 125 )
              return 0.75;
          if( aVoltage <= 160 )
              return 0.80;
          if( aVoltage <= 200 )
              return 1.00;
          if( aVoltage <= 250 )
              return 1.25;
          if( aVoltage <= 320 )
              return 1.60;
          if( aVoltage <= 400 )
              return 2.0;
          if( aVoltage <= 500 )
              return 2.5;
          if( aVoltage <= 630 )
              return 3.2;
          if( aVoltage <= 800 )
              return 4.0;
          if( aVoltage <= 1000 )
              return 5.0;
          if( aVoltage <= 1250 )
              return 6.3;
          if( aVoltage <= 1600 )
              return 8.0;
          if( aVoltage <= 2000 )
              return 10.0;
          if( aVoltage <= 2500 )
              return 12.5;
          if( aVoltage <= 3200 )
              return 16.0;
          if( aVoltage <= 4000 )
              return 20.0;
          if( aVoltage <= 5000 )
              return 25.0;
          if( aVoltage <= 6300 )
              return 32.0;
          if( aVoltage <= 8000 )
              return 40.0;
          if( aVoltage <= 10000 )
              return 50.0;
          if( aVoltage <= 12500 )
              return 63.0;
          if( aVoltage <= 16000 )
              return 80.0;
          if( aVoltage <= 20000 )
              return 100.0;
          if( aVoltage <= 25000 )
              return 125.0;
          if( aVoltage <= 32000 )
              return 160.0;
          if( aVoltage <= 40000 )
              return 200.0;
          if( aVoltage <= 50000 )
              return 250.0;
          if( aVoltage <= 63000 )
              return 320.0;
      }
      if( aPD == POLLUTION_DEGREE.PD2 && aMG == MATERIAL_GROUP.MG_II )
      {
          if( aVoltage <= 10 )
              return 0.400;
          if( aVoltage <= 12.5 )
              return 0.420;
          if( aVoltage <= 16 )
              return 0.450;
          if( aVoltage <= 20 )
              return 0.480;
          if( aVoltage <= 25 )
              return 0.500;
          if( aVoltage <= 32 )
              return 0.53;
          if( aVoltage <= 40 )
              return 0.80;
          if( aVoltage <= 50 )
              return 0.85;
          if( aVoltage <= 63 )
              return 0.90;
          if( aVoltage <= 80 )
              return 0.95;
          if( aVoltage <= 100 )
              return 1.00;
          if( aVoltage <= 125 )
              return 1.05;
          if( aVoltage <= 160 )
              return 1.10;
          if( aVoltage <= 200 )
              return 1.40;
          if( aVoltage <= 250 )
              return 1.80;
          if( aVoltage <= 320 )
              return 2.20;
          if( aVoltage <= 400 )
              return 2.8;
          if( aVoltage <= 500 )
              return 3.6;
          if( aVoltage <= 630 )
              return 4.5;
          if( aVoltage <= 800 )
              return 5.6;
          if( aVoltage <= 1000 )
              return 7.1;
          if( aVoltage <= 1250 )
              return 9.0;
          if( aVoltage <= 1600 )
              return 11.0;
          if( aVoltage <= 2000 )
              return 14.0;
          if( aVoltage <= 2500 )
              return 18.0;
          if( aVoltage <= 3200 )
              return 22.0;
          if( aVoltage <= 4000 )
              return 28.0;
          if( aVoltage <= 5000 )
              return 36.0;
          if( aVoltage <= 6300 )
              return 45.0;
          if( aVoltage <= 8000 )
              return 56.0;
          if( aVoltage <= 10000 )
              return 71.0;
          if( aVoltage <= 12500 )
              return 90.0;
          if( aVoltage <= 16000 )
              return 110.0;
          if( aVoltage <= 20000 )
              return 140.0;
          if( aVoltage <= 25000 )
              return 180.0;
          if( aVoltage <= 32000 )
              return 220.0;
          if( aVoltage <= 40000 )
              return 280.0;
          if( aVoltage <= 50000 )
              return 360.0;
          if( aVoltage <= 63000 )
              return 450.0;
      }
      if( aPD == POLLUTION_DEGREE.PD2
          && ( aMG == MATERIAL_GROUP.MG_IIIa || aMG == MATERIAL_GROUP.MG_IIIb ) )
      {
          if( aVoltage <= 10 )
              return 0.400;
          if( aVoltage <= 12.5 )
              return 0.420;
          if( aVoltage <= 16 )
              return 0.450;
          if( aVoltage <= 20 )
              return 0.480;
          if( aVoltage <= 25 )
              return 0.500;
          if( aVoltage <= 32 )
              return 0.53;
          if( aVoltage <= 40 )
              return 1.10;
          if( aVoltage <= 50 )
              return 1.20;
          if( aVoltage <= 63 )
              return 1.25;
          if( aVoltage <= 80 )
              return 1.30;
          if( aVoltage <= 100 )
              return 1.40;
          if( aVoltage <= 125 )
              return 1.50;
          if( aVoltage <= 160 )
              return 1.60;
          if( aVoltage <= 50000 )
              return aVoltage / 100;
          if( aVoltage <= 63000 )
              return 600.0;
      }
      if( aPD == POLLUTION_DEGREE.PD3 && aMG == MATERIAL_GROUP.MG_I )
      {
          if( aVoltage <= 10 )
              return 1.000;
          if( aVoltage <= 12.5 )
              return 1.050;
          if( aVoltage <= 16 )
              return 1.100;
          if( aVoltage <= 20 )
              return 1.200;
          if( aVoltage <= 25 )
              return 1.250;
          if( aVoltage <= 32 )
              return 1.30;
          if( aVoltage <= 40 )
              return 1.40;
          if( aVoltage <= 50 )
              return 1.50;
          if( aVoltage <= 63 )
              return 1.60;
          if( aVoltage <= 80 )
              return 1.70;
          if( aVoltage <= 100 )
              return 1.80;
          if( aVoltage <= 125 )
              return 1.90;
          if( aVoltage <= 160 )
              return 2.00;
          if( aVoltage <= 200 )
              return 2.50;
          if( aVoltage <= 250 )
              return 3.20;
          if( aVoltage <= 320 )
              return 4.00;
          if( aVoltage <= 400 )
              return 5.0;
          if( aVoltage <= 500 )
              return 6.3;
          if( aVoltage <= 630 )
              return 8.0;
          if( aVoltage <= 800 )
              return 10.0;
          if( aVoltage <= 1000 )
              return 12.5;
          if( aVoltage <= 1250 )
              return 16.0;
          if( aVoltage <= 1600 )
              return 20.0;
          if( aVoltage <= 2000 )
              return 25.0;
          if( aVoltage <= 2500 )
              return 32.0;
          if( aVoltage <= 3200 )
              return 40.0;
          if( aVoltage <= 4000 )
              return 50.0;
          if( aVoltage <= 5000 )
              return 63.0;
          if( aVoltage <= 6300 )
              return 80.0;
          if( aVoltage <= 8000 )
              return 100.0;
          if( aVoltage <= 10000 )
              return 125.0;
      }
      if( aPD == POLLUTION_DEGREE.PD3 && aMG == MATERIAL_GROUP.MG_II )
      {
          if( aVoltage <= 10 )
              return 1.000;
          if( aVoltage <= 12.5 )
              return 1.050;
          if( aVoltage <= 16 )
              return 1.100;
          if( aVoltage <= 20 )
              return 1.200;
          if( aVoltage <= 25 )
              return 1.250;
          if( aVoltage <= 32 )
              return 1.30;
          if( aVoltage <= 40 )
              return 1.60;
          if( aVoltage <= 50 )
              return 1.70;
          if( aVoltage <= 63 )
              return 1.80;
          if( aVoltage <= 80 )
              return 1.90;
          if( aVoltage <= 100 )
              return 2.00;
          if( aVoltage <= 125 )
              return 2.10;
          if( aVoltage <= 160 )
              return 2.20;
          if( aVoltage <= 200 )
              return 2.80;
          if( aVoltage <= 250 )
              return 3.60;
          if( aVoltage <= 320 )
              return 4.50;
          if( aVoltage <= 400 )
              return 5.6;
          if( aVoltage <= 500 )
              return 7.1;
          if( aVoltage <= 630 )
              return 9.0;
          if( aVoltage <= 800 )
              return 11.0;
          if( aVoltage <= 1000 )
              return 14.0;
          if( aVoltage <= 1250 )
              return 18.0;
          if( aVoltage <= 1600 )
              return 22.0;
          if( aVoltage <= 2000 )
              return 28.0;
          if( aVoltage <= 2500 )
              return 36.0;
          if( aVoltage <= 3200 )
              return 45.0;
          if( aVoltage <= 4000 )
              return 56.0;
          if( aVoltage <= 5000 )
              return 71.0;
          if( aVoltage <= 6300 )
              return 90.0;
          if( aVoltage <= 8000 )
              return 110.0;
          if( aVoltage <= 10000 )
              return 140.0;
      }
      if( aPD == POLLUTION_DEGREE.PD3
          && ( aMG == MATERIAL_GROUP.MG_IIIa || aMG == MATERIAL_GROUP.MG_IIIb ) )
      {
          if( aVoltage <= 10 )
              return 1.000;
          if( aVoltage <= 12.5 )
              return 1.050;
          if( aVoltage <= 16 )
              return 1.100;
          if( aVoltage <= 20 )
              return 1.200;
          if( aVoltage <= 25 )
              return 1.250;
          if( aVoltage <= 32 )
              return 1.30;
          if( aVoltage <= 40 )
              return 1.80;
          if( aVoltage <= 50 )
              return 1.90;
          if( aVoltage <= 63 )
              return 2.00;
          if( aVoltage <= 80 )
              return 2.10;
          if( aVoltage <= 100 )
              return 2.20;
          if( aVoltage <= 125 )
              return 2.40;
          if( aVoltage <= 160 )
              return 2.50;
          if( aVoltage <= 200 )
              return 3.20;
          if( aVoltage <= 250 )
              return 4.00;
          if( aVoltage <= 320 )
              return 5.00;
          if( aVoltage <= 400 )
              return 6.3;
          if( aVoltage <= 500 )
              return 8.0;
          if( aVoltage <= 630 )
              return 10.0;
          if( aVoltage <= 800 )
              return 12.5;
          if( aVoltage <= 1000 )
              return 16.0;
          if( aVoltage <= 1250 )
              return 20.0;
          if( aVoltage <= 1600 )
              return 25.0;
          if( aVoltage <= 2000 )
              return 32.0;
          if( aVoltage <= 2500 )
              return 40.0;
          if( aVoltage <= 3200 )
              return 50.0;
          if( aVoltage <= 4000 )
              return 63.0;
          if( aVoltage <= 5000 )
              return 80.0;
          if( aVoltage <= 6300 )
              return 100.0;
          if( aVoltage <= 8000 )
              return 125.0;
          if( aVoltage <= 10000 )
              return 160.0;
      }
  
      return -1; // Pointed out of the table.
  }

  /** `IEC60664::ComputeClearanceDistance`. */
  ComputeClearanceDistance(aPD: number, aField: number, aAltitude: number): number {
      //  Based on IEC60664-1 : 2020-05 chart G.1
  
  
  
      let transientVoltage = this.m_transientVoltage;
  
      // IEC60664-1 : 2020-05 : 5.2.5
  
      // Preferred series has a specific rule
      if( this.m_insulationType == INSULATION_TYPE.REINFORCED )
      {
          if( transientVoltage == 0.33 )
              transientVoltage = 0.5;
          else if( transientVoltage == 0.5 )
              transientVoltage = 0.8;
          else if( transientVoltage == 0.8 )
              transientVoltage = 1.5;
          else if( transientVoltage == 1.5 )
              transientVoltage = 2.5;
          else if( transientVoltage == 2.5 )
              transientVoltage = 4;
          else if( transientVoltage == 4 )
              transientVoltage = 6;
          else if( transientVoltage == 6 )
              transientVoltage = 8;
          else if( transientVoltage == 8 )
              transientVoltage = 12;
          else
              transientVoltage = transientVoltage * 1.6;
      }
  
      let clearance1 = this.GetClearanceToWithstandTransientVoltage( transientVoltage, aPD, aField );
      let clearance2 = this.GetClearanceToWithstandPeaks(
              ( this.m_insulationType == INSULATION_TYPE.REINFORCED ) ? this.m_peakVoltage * 1.6
                                                                  : this.m_peakVoltage,
              aField );
  
      if( ( clearance1 == -1 ) || ( clearance2 == -1 ) )
      {
          clearance1 = -1;
          clearance2 = -1;
      }
  
      let clearance = ( clearance1 > clearance2 ) ? clearance1 : clearance2;
      clearance *= this.GetClearanceAltitudeCorrectionFactor( aAltitude );
      return clearance;
  }

  /** `IEC60664::ComputeCreepageDistance`. */
  ComputeCreepageDistance(aPD: number, aMG: number): number {
      //  Based on IEC60664-1 : 2020-05 chart H.1
  
  
  
      let creepage = this.GetBasicCreepageDistance( this.m_RMSvoltage, aPD, aMG );
  
      if( this.m_insulationType == INSULATION_TYPE.REINFORCED )
      {
          creepage *= 2;
      }
      return creepage;
  }

  /** `IEC60664::Compute`. */
  Compute(): boolean {
      this.m_clearance = this.ComputeClearanceDistance( this.m_pollutionDegree, this.m_field, this.m_altitude );
      this.m_creepage = this.ComputeCreepageDistance( this.m_pollutionDegree, this.m_materialGroup );
  
      if( this.m_creepage < this.m_clearance || this.m_clearance <= 0 )
          this.m_creepage = this.m_clearance;
  
      this.m_minGrooveWidth = this.GetMinGrooveWidthFor( this.m_pollutionDegree, this.m_clearance );
  
      return true;
  }

}
