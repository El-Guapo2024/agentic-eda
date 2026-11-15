// The Calculator Tools' tree (`PCB_CALCULATOR_FRAME::loadPages`, KiCad 25ec2335): five group rows, each followed by its calculators, in KiCad's
// order. A row's index is its wxTreebook page index, which `last_page` stores (the group rows count: 1 is Regulators).
import type { ComponentType } from "react";
import { PanelRegulator } from "./PanelRegulator";
import { PanelRCalculator } from "./PanelRCalculator";
import { PanelPthSize } from "./PanelPthSize";
import { PanelElectricalSpacing } from "./PanelElectricalSpacing";
import { PanelViaSize } from "./PanelViaSize";
import { PanelTrackWidth } from "./PanelTrackWidth";
import { PanelFusingCurrent } from "./PanelFusingCurrent";
import { PanelCableSize } from "./PanelCableSize";
import { PanelWavelength } from "./PanelWavelength";
import { PanelRfAttenuators } from "./PanelRfAttenuators";
import { PanelTransline } from "./PanelTransline";
import { PanelESeries } from "./PanelESeries";
import { PanelColorCode } from "./PanelColorCode";
import { PanelBoardClass } from "./PanelBoardClass";
import { PanelGalvanicCorrosion } from "./PanelGalvanicCorrosion";

export interface CalcPage {
  label: string;
  /** A stable name for the page (the KIWAY mail's `page`, "pth_size", and the browser checks' test ids). */
  id: string;
  /** Null for a group row (`m_treebook->AddPage( nullptr, .. )`). */
  component: ComponentType | null;
}

const group = (label: string, id: string): CalcPage => ({ label, id, component: null });

export const CALC_PAGES: readonly CalcPage[] = [
  group("General system design", "group-general"),
  { label: "Regulators", id: "regulators", component: PanelRegulator },
  { label: "Resistor Calculator", id: "r_calculator", component: PanelRCalculator },
  group("Footprint design", "group-footprint"),
  { label: "Through-hole Size", id: "pth_size", component: PanelPthSize },
  group("Power, current and isolation", "group-power"),
  { label: "Electrical Spacing", id: "electrical_spacing", component: PanelElectricalSpacing },
  { label: "Via Size", id: "via_size", component: PanelViaSize },
  { label: "Track Width", id: "track_width", component: PanelTrackWidth },
  { label: "Fusing Current", id: "fusing_current", component: PanelFusingCurrent },
  { label: "Cable Size", id: "cable_size", component: PanelCableSize },
  group("High Speed", "group-highspeed"),
  { label: "Wavelength", id: "wavelength", component: PanelWavelength },
  { label: "RF Attenuators", id: "rf_attenuators", component: PanelRfAttenuators },
  { label: "Transmission Lines", id: "transline", component: PanelTransline },
  group("Memo", "group-memo"),
  { label: "E-Series", id: "eseries", component: PanelESeries },
  { label: "Color Code", id: "color_code", component: PanelColorCode },
  { label: "Board Classes", id: "board_class", component: PanelBoardClass },
  { label: "Galvanic Corrosion", id: "galvanic_corrosion", component: PanelGalvanicCorrosion },
];

export function calcPageIndex(id: string): number {
  return CALC_PAGES.findIndex((p) => p.id === id);
}

/**
 * The page a tree index shows: `OnPageChanged` moves a click on a group row to the group's first calculator (`ChangeSelection( page + 1 )`), and a
 * stored index out of range is `ChangeSelection`'s no-op, the first calculator.
 */
export function shownPageIndex(index: number): number {
  const i = Number.isInteger(index) && index >= 0 && index < CALC_PAGES.length ? index : 1;
  return CALC_PAGES[i]!.component ? i : Math.min(i + 1, CALC_PAGES.length - 1);
}
