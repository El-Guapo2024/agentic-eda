// The shapes of the board's barcodes, fetched: the server plots a barcode with kicad-cli the first time it is asked for its shape and answers `pending` until the plot is done
// (crates/cli/src/barcode_api.rs), so this asks again, and a version number moves when a shape has arrived so the canvas that asked repaints (a canvas draws synchronously and cannot
// wait). The shapes themselves are kept by kicad-port/barcodeShapes.ts, which the pickers read.
import { useSyncExternalStore } from "react";
import type { BoardBarcode, CmdBarcode } from "../../api/pcbItemTypes";
import { barcodeShape, barcodeShapeKey, setBarcodeShape, type BarcodePath } from "../../kicad-port/barcodeShapes";

let version = 0;
const listeners = new Set<() => void>();
const asked = new Set<string>();
/** How long to wait between two asks while a plot runs, and how many asks give up (a plot takes a fraction of a second). */
const RETRY_MS = 300;
const MAX_ASKS = 120;

function bump(): void {
  version++;
  for (const l of listeners) l();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The number of shapes that have arrived: a canvas that draws a barcode lists it among what it repaints for. */
export function useBarcodeVersion(): number {
  return useSyncExternalStore(subscribe, () => version);
}

export type BarcodeReply = { status: "ready"; paths: BarcodePath[] } | { status: "pending" } | { status: "failed"; error: string };

function asReply(j: unknown): BarcodeReply {
  const o = (j ?? {}) as { status?: string; paths?: BarcodePath[]; error?: string };
  if (o.status === "ready") return { status: "ready", paths: o.paths ?? [] };
  if (o.status === "failed") return { status: "failed", error: o.error ?? "The barcode could not be drawn." };
  return { status: "pending" };
}

/** Ask for the shape of `b` unless it is known or being asked for: the canvas calls this for every barcode it draws, on every repaint. */
export function requestBarcodeShape(b: BoardBarcode): void {
  const key = barcodeShapeKey(b);
  if (barcodeShape(key)?.status === "ready" || barcodeShape(key)?.status === "failed" || asked.has(key)) return;
  asked.add(key);
  const url = `/api/barcode_shape?id=${encodeURIComponent(b.id)}`;
  const ask = async (attempt: number): Promise<void> => {
    let reply: BarcodeReply;
    try {
      reply = asReply(await fetch(url).then((r) => r.json()));
    } catch {
      reply = { status: "failed", error: "The studio did not answer." };
    }
    if (reply.status === "pending") {
      if (attempt < MAX_ASKS) {
        setBarcodeShape(key, { status: "pending" });
        setTimeout(() => void ask(attempt + 1), RETRY_MS);
        return;
      }
      reply = { status: "failed", error: "The barcode took too long to draw." };
    }
    setBarcodeShape(key, reply);
    asked.delete(key);
    bump();
  };
  void ask(0);
}

/**
 * The shape of a barcode that is not on the board yet (Barcode Properties' preview, `DIALOG_BARCODE_PROPERTIES::refreshPreview`): the server plots it as it plots one on the board.
 * Resolves with `pending` while it runs; the dialog asks again.
 */
export async function previewBarcode(barcode: CmdBarcode): Promise<BarcodeReply> {
  try {
    const r = await fetch("/api/barcode_preview", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ barcode }) });
    return asReply(await r.json());
  } catch {
    return { status: "failed", error: "The studio did not answer." };
  }
}
