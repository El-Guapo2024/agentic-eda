// "Allow free pads" (`PCBNEW_SETTINGS::m_AllowFreePads`, Preferences > PCB Editor > Editing Options; UI-LEFT-3 R3-7): "pads can be moved with respect to the rest
// of the footprint". The edit tools' `PCB_SELECTION_TOOL::FilterCollectorForFreePads( aCollector, aForcePromotion )` promotes a pad to its footprint unless it is
// on (`!m_AllowFreePads || aForcePromotion`): Move, Rotate, Mirror, Move Exactly, Position Relative, Swap and Align/Distribute then work on a pad as itself, and the
// Properties panel's Position X / Y moves the pad, not its footprint (`PCB_PROPERTIES_PANEL::valueChanged`). Duplicate always promotes (`aForcePromotion`).
// The store sets it from the preferences; the pure kicad-port modules read it.

let allow = false;

/** `PCBNEW_SETTINGS::m_AllowFreePads`. */
export function getAllowFreePads(): boolean {
  return allow;
}

export function setAllowFreePads(on: boolean): void {
  allow = on;
}

/** `FilterCollectorForFreePads`' test for one pad: whether the edit tools take it as itself (`!m_isFootprintEditor && ( !m_AllowFreePads || aForcePromotion )` is false). */
export function padStaysPad(forcePromotion = false): boolean {
  return allow && !forcePromotion;
}
