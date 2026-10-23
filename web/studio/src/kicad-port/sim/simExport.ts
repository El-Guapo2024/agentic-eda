// `SIMULATOR_CONTROL::ExportPlotToSchematic` (eeschema/tools/simulator_control.cpp, commit 8303b2ad): the plot's PNG as the clipboard text of a schematic
// image -- "(image (at 0 0)", "  (data", the base64 in quoted lines of 76 characters (MIME_BASE64_LENGTH), "  )", ")" -- which the schematic's Paste reads.

const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/** `wxBase64Encode`. */
export function base64(bytes: Uint8Array): string {
  let out = "";
  for (let i = 0; i < bytes.length; i += 3) {
    const a = bytes[i]!;
    const b = bytes[i + 1];
    const c = bytes[i + 2];
    out += B64[a >> 2]! + B64[((a & 3) << 4) | ((b ?? 0) >> 4)]!;
    out += b === undefined ? "=" : B64[((b & 15) << 2) | ((c ?? 0) >> 6)]!;
    out += c === undefined ? "=" : B64[c & 63]!;
  }
  return out;
}

export function plotImageSexpr(png: Uint8Array): string {
  const out = base64(png);
  let s = "(image (at 0 0)\n  (data\n";
  for (let first = 0; first < out.length; first += 76) s += `    "${out.slice(first, first + 76)}"\n`;
  s += "  )\n)\n";
  return s;
}
