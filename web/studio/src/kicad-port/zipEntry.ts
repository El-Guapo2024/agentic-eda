// One file out of a ZIP archive, in the browser: an EasyEDA Pro project (`.epro`, or a `.zip`) is a ZIP whose `project.json` lists its boards and schematics, which the
// chooser of an import shows (`EASYEDAPRO::ReadProjectOrDeviceFile`, `FindJsonFile`: KiCad reads it with wxZipInputStream). The central directory says where each entry is; a
// stored entry is its bytes, a deflated one goes through the browser's own `DecompressionStream("deflate-raw")` (no new dependency). Unit-tested in zipEntry.test.ts.

const u16 = (b: Uint8Array, o: number) => (b[o] ?? 0) | ((b[o + 1] ?? 0) << 8);
const u32 = (b: Uint8Array, o: number) => (u16(b, o) | (u16(b, o + 2) << 16)) >>> 0;

/** Inflate raw DEFLATE data with the platform's `DecompressionStream`. */
async function inflateRaw(data: Uint8Array): Promise<Uint8Array> {
  const stream = new Blob([data]).stream().pipeThrough(new DecompressionStream("deflate-raw"));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

/** The bytes of the entry named `wanted` (a name at the archive's root or a path in it), or `null` when the archive has none or is not a ZIP this reads (stored or deflated). */
export async function readZipEntry(zip: Uint8Array, wanted: string): Promise<Uint8Array | null> {
  // the end of central directory record: its signature in the last 64 KiB + 22 bytes
  let eocd = -1;
  for (let i = zip.length - 22; i >= Math.max(0, zip.length - 22 - 0xffff); i--) {
    if (u32(zip, i) === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) return null;
  const count = u16(zip, eocd + 10);
  let at = u32(zip, eocd + 16);
  const decoder = new TextDecoder();
  for (let n = 0; n < count && at + 46 <= zip.length; n++) {
    if (u32(zip, at) !== 0x02014b50) return null;
    const method = u16(zip, at + 10);
    const size = u32(zip, at + 20);
    const nameLength = u16(zip, at + 28);
    const extraLength = u16(zip, at + 30);
    const commentLength = u16(zip, at + 32);
    const local = u32(zip, at + 42);
    const name = decoder.decode(zip.subarray(at + 46, at + 46 + nameLength));
    at += 46 + nameLength + extraLength + commentLength;
    if (name !== wanted) continue;
    if (u32(zip, local) !== 0x04034b50) return null;
    const start = local + 30 + u16(zip, local + 26) + u16(zip, local + 28);
    const data = zip.subarray(start, start + size);
    if (method === 0) return data.slice();
    if (method === 8) return inflateRaw(data);
    return null;
  }
  return null;
}

/** `project.json` of an EasyEDA Pro project file given as base64, parsed; `null` when there is none (another format, KiCad 10's v3 `.epro2`, or a file this cannot read). */
export async function easyEdaProject(base64: string): Promise<unknown | null> {
  try {
    const binary = atob(base64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
    const entry = await readZipEntry(bytes, "project.json");
    return entry ? (JSON.parse(new TextDecoder().decode(entry)) as unknown) : null;
  } catch {
    return null;
  }
}
