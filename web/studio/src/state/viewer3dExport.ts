// Whether the 3D viewer's "Export 3D View" dialog (`DIALOG_EXPORT_3D_IMAGE`, components/viewer3d/Export3DImageDialog.tsx) is open: Export Image... from the toolbar or
// its key in the 3D view opens it. Plain module state with subscribers.
import { useSyncExternalStore } from "react";

let open = false;
const listeners = new Set<() => void>();
const emit = () => {
  for (const l of [...listeners]) l();
};

export function openExport3DImage(): void {
  open = true;
  emit();
}

export function closeExport3DImage(): void {
  open = false;
  emit();
}

export function useExport3DImageOpen(): boolean {
  return useSyncExternalStore(
    (l) => (listeners.add(l), () => listeners.delete(l)),
    () => open,
    () => open
  );
}
