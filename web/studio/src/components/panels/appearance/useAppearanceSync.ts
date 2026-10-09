// Keeps the Appearance panel's settings per project: reads `appearance.json` once when the studio opens and lays it over the state (`APPEARANCE_LOAD`), then
// writes the file again, debounced, whenever anything the file holds changes -- KiCad's `LoadProjectSettings` / `SaveProjectLocalSettings`, with the file in the
// project's folder instead of beside a `.kicad_pro`. Nothing is written before both the file and the board have been read, so a board that has not loaded yet
// (and so has no layers or nets to name) cannot overwrite what the file says about them.
import { useEffect, useMemo, useRef } from "react";
import { useStudioDispatch, useStudioState } from "../../../state/store";
import { fetchAppearance, postAppearance } from "../../../api/appearanceClient";
import { netsContext, hiddenNetsOnLoad } from "../../../kicad-port/appearanceNets";
import { toAppearanceFile } from "../../../kicad-port/appearanceFile";
import type { ViewSlice } from "../../../kicad-port/appearanceOps";

const SAVE_DELAY_MS = 500;

export function useAppearanceSync(): void {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const loaded = useRef(false);
  const classesExpanded = useRef(false);
  const lastSaved = useRef<string | null>(null);

  // Read the file once.
  useEffect(() => {
    let alive = true;
    fetchAppearance()
      .then((file) => {
        if (!alive) return;
        dispatch({ type: "APPEARANCE_LOAD", file });
        loaded.current = true;
      })
      .catch(() => {
        // No server (or an old one): the settings simply are not kept.
      });
    return () => {
      alive = false;
    };
  }, [dispatch]);

  const board = state.board;
  const nets = useMemo(() => netsContext(board), [board]);

  // `LoadProjectSettings`: the nets of a hidden class are hidden too -- once the board's nets are known.
  const hiddenClasses = state.appearance.hiddenNetclasses;
  const hiddenNets = state.bcx.hiddenRatsnestNets;
  useEffect(() => {
    if (!loaded.current || !board || classesExpanded.current || nets.nets.length === 0) return;
    classesExpanded.current = true;
    const next = hiddenNetsOnLoad(hiddenNets, hiddenClasses, nets);
    if (next.length !== hiddenNets.length || next.some((n) => !hiddenNets.includes(n))) dispatch({ type: "BCX", patch: { hiddenRatsnestNets: next } });
  }, [board, nets, hiddenClasses, hiddenNets, dispatch, loaded.current]); // eslint-disable-line react-hooks/exhaustive-deps

  // What the file holds, as text, recomputed only when one of the fields it is made of changes.
  const slice: ViewSlice = useMemo(
    () => ({
      appearance: state.appearance,
      layerVisible: state.layerVisible,
      layerOpacity: state.layerOpacity,
      activeLayer: state.activeLayer,
      highContrast: state.highContrast,
      showRatsnest: state.showRatsnest,
      gridVisible: state.gridVisible,
      boardFlipped: state.bcx.boardFlipped,
      ratsnestMode: state.bcx.ratsnestMode,
      hiddenNets: state.bcx.hiddenRatsnestNets,
    }),
    [state.appearance, state.layerVisible, state.layerOpacity, state.activeLayer, state.highContrast, state.showRatsnest, state.gridVisible, state.bcx.boardFlipped, state.bcx.ratsnestMode, state.bcx.hiddenRatsnestNets]
  );
  const file = useMemo(() => (board && loaded.current ? toAppearanceFile(slice, nets) : null), [slice, nets, board, loaded.current]); // eslint-disable-line react-hooks/exhaustive-deps
  const text = useMemo(() => (file ? JSON.stringify({ local: file.local, project: file.project }) : null), [file]);

  useEffect(() => {
    if (!file || text === null) return;
    // The first text after loading is what the file already says (or the defaults): the baseline, not a change.
    if (lastSaved.current === null) {
      lastSaved.current = text;
      return;
    }
    if (text === lastSaved.current) return;
    const timer = setTimeout(() => {
      // Remembered once the server has it: a write that failed is tried again with the next change.
      postAppearance(file)
        .then(() => {
          lastSaved.current = text;
        })
        .catch(() => {});
    }, SAVE_DELAY_MS);
    return () => clearTimeout(timer);
  }, [file, text]);
}
