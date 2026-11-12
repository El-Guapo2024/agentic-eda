// File drops (`EDA_BASE_FRAME::OnDropFiles`, common/eda_base_frame.cpp): a file dropped on the window runs the action its editor's `m_acceptedExts` names for its extension --
// Append Board and Import Graphics on the board, Import Footprint in the Footprint Editor, Import Sheet and Import Graphics in the schematic editor (kicad-port/fileDrops.ts has the tables).
// KiCad hands the action the file's path; a browser hands the page the file's contents, so the file is read here and the action is run with `{ name, text }`.
//
// Several files dropped together run one after the other, in the drop's order (`DoWithAcceptedFiles`), each once the one before is done with the frame.
//
// Mounted once by App.tsx. It also keeps the browser from doing what it does with a file dropped on a page nobody listens to (leave for the file). A drop while a dialog is open is
// ignored, as a modal dialog blocks KiCad's frame, except on Import Graphics, which takes the file into its File: field. A dashed outline shows while a file is over the window.
import { useEffect, useRef, useState } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { useActionRunner } from "../actions/useActionRunner";
import { dropMessage, planDrop, type DropRun } from "../kicad-port/fileDrops";
import { getImportPreview } from "../state/importPreview";
import { getSchCarry } from "../state/schCarryStore";
import { getSchImageHand } from "../state/schImageHand";
import { getImportGraphicsDialog } from "../state/importDialogs";
import { MAX_IMPORT_BYTES } from "../api/libraryClient";
import { readImageFile } from "../actions/imageFiles";

const hasFiles = (e: DragEvent): boolean => !!e.dataTransfer && Array.from(e.dataTransfer.types).includes("Files");

export function FileDropHost() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const { run, isEnabled } = useActionRunner();
  const [over, setOver] = useState(false);
  // The handlers are installed once; they read the latest of these.
  const latest = useRef({ tab: state.tab, run, isEnabled, dispatch, state });
  latest.current = { tab: state.tab, run, isEnabled, dispatch, state };

  useEffect(() => {
    let depth = 0;
    const onDragEnter = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      depth++;
      setOver(true);
    };
    const onDragLeave = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      depth = Math.max(0, depth - 1);
      if (depth === 0) setOver(false);
    };
    const onDragOver = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      e.preventDefault(); // without this the browser would take the drop for itself
      if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    };
    const onDrop = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      depth = 0;
      setOver(false);
      const files = Array.from(e.dataTransfer?.files ?? []);
      if (files.length === 0) return;
      void openDropped(files);
    };
    const openDropped = async (files: File[]) => {
      const { tab, isEnabled, dispatch } = latest.current;
      const toast = (message: string, kind: "info" | "error" = "info") => dispatch({ type: "TOAST", message, kind });
      // A dialog holds the frame, as a modal one does in KiCad; Import Graphics alone takes a graphic dropped outside its own drop zone, into its File: field.
      const importOpen = !!getImportGraphicsDialog();
      if (document.querySelector(".dialog") && !importOpen) return;
      const plan = planDrop(tab, files.map((f) => f.name), (action) => isEnabled(action) && (!importOpen || action.endsWith("ddImportGraphics")));
      const word = dropMessage(tab, plan);
      if (word) toast(word);
      // `DoWithAcceptedFiles`: each accepted file runs its action in turn. KiCad's `RunAction` returns once the action's dialog or interactive tool is done with the frame;
      // here the next file waits until the frame is free again (no dialog, nothing in the hand, no preview riding the cursor). A drop on an open Import Graphics dialog
      // takes the first file only: it fills the dialog's one File: field.
      const runs = importOpen ? plan.runs.slice(0, 1) : plan.runs;
      for (let i = 0; i < runs.length; i++) {
        if (i > 0 && !(await frameFreed())) return;
        await runDropped(runs[i]!, files, toast);
      }
    };
    const runDropped = async (r: DropRun, files: File[], toast: (message: string, kind?: "info" | "error") => void) => {
      const { run } = latest.current;
      const file = files.find((f) => f.name === r.name)!;
      // An image (`ddAddImage`) is bytes, not text: it is handed over as the base64 the placement carries.
      if (r.action.endsWith("ddAddImage")) {
        try {
          run(r.action, await readImageFile(file));
        } catch (err) {
          toast(err instanceof Error ? err.message : String(err), "error");
        }
        return;
      }
      // An action that takes every file of its kind (the schematics Import Sheet is dropped with) is handed all of them, with their text.
      if (r.names) {
        const group = files.filter((f) => r.names!.includes(f.name));
        const big = group.find((f) => f.size > MAX_IMPORT_BYTES);
        if (big) return toast(`${big.name} is larger than 64 MB, which the studio's server will not read`, "error");
        try {
          run(r.action, { files: await Promise.all(group.map(async (f) => ({ name: f.name, text: await f.text() }))) });
        } catch (err) {
          toast(err instanceof Error ? err.message : String(err), "error");
        }
        return;
      }
      if (file.size > MAX_IMPORT_BYTES) return toast(`${file.name} is larger than 64 MB, which the studio's server will not read`, "error");
      let text: string;
      try {
        text = await file.text();
      } catch (err) {
        return toast(err instanceof Error ? err.message : String(err), "error");
      }
      run(r.action, { name: file.name, text });
    };
    /** Is the frame in use: a dialog open, something carried or previewed, a sheet waiting for its clicks, a tool other than selection running? */
    const frameBusy = (): boolean => {
      const st = latest.current.state;
      return (
        !!document.querySelector(".dialog") ||
        st.movePreview != null ||
        st.schSheetImport != null ||
        st.armedFootprint != null ||
        getImportPreview() != null ||
        getSchCarry() != null ||
        getSchImageHand() != null
      );
    };
    /**
     * Waits for the action just run to be done with the frame: it shows its dialog or its carry once its file is read (a round trip to the server), so the wait first gives it
     * the time to start (up to 4 s), then waits for the frame to be free. False when the tab changed meanwhile, which ends the drop as leaving KiCad's frame would.
     */
    const frameFreed = async (): Promise<boolean> => {
      const tab = latest.current.tab;
      const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
      for (let waited = 0; waited < 4000 && !frameBusy(); waited += 100) await sleep(100);
      for (let idle = 0; idle < 3; ) {
        await sleep(150);
        if (latest.current.tab !== tab) return false;
        idle = frameBusy() ? 0 : idle + 1;
      }
      return true;
    };
    window.addEventListener("dragenter", onDragEnter);
    window.addEventListener("dragleave", onDragLeave);
    window.addEventListener("dragover", onDragOver);
    window.addEventListener("drop", onDrop);
    return () => {
      window.removeEventListener("dragenter", onDragEnter);
      window.removeEventListener("dragleave", onDragLeave);
      window.removeEventListener("dragover", onDragOver);
      window.removeEventListener("drop", onDrop);
    };
  }, []);

  if (!over) return null;
  return <div aria-hidden data-testid="file-drop-outline" style={{ position: "fixed", inset: 6, border: "2px dashed var(--accent, #4a9eff)", borderRadius: 8, pointerEvents: "none", zIndex: 2000 }} />;
}
