import { test } from "node:test";
import assert from "node:assert/strict";
import { easyEdaProject, readZipEntry } from "./zipEntry";

/** A ZIP of `entries` (each stored, or deflated with the platform's CompressionStream), as a writer lays it out: local headers, then the central directory, then its end. */
async function zipOf(entries: Array<{ name: string; text: string; deflate: boolean }>): Promise<Uint8Array> {
  const enc = new TextEncoder();
  const parts: number[] = [];
  const central: number[] = [];
  const le = (out: number[], n: number, bytes: number) => {
    for (let i = 0; i < bytes; i++) out.push((n >>> (8 * i)) & 0xff);
  };
  for (const e of entries) {
    const raw = enc.encode(e.text);
    const data = e.deflate ? new Uint8Array(await new Response(new Blob([raw]).stream().pipeThrough(new CompressionStream("deflate-raw"))).arrayBuffer()) : raw;
    const name = enc.encode(e.name);
    const offset = parts.length;
    le(parts, 0x04034b50, 4);
    le(parts, 20, 2);
    le(parts, 0, 2);
    le(parts, e.deflate ? 8 : 0, 2);
    le(parts, 0, 4); // time, date
    le(parts, 0, 4); // crc (not checked)
    le(parts, data.length, 4);
    le(parts, raw.length, 4);
    le(parts, name.length, 2);
    le(parts, 0, 2);
    parts.push(...name, ...data);
    le(central, 0x02014b50, 4);
    le(central, 20, 2);
    le(central, 20, 2);
    le(central, 0, 2);
    le(central, e.deflate ? 8 : 0, 2);
    le(central, 0, 4);
    le(central, 0, 4);
    le(central, data.length, 4);
    le(central, raw.length, 4);
    le(central, name.length, 2);
    le(central, 0, 2);
    le(central, 0, 2);
    le(central, 0, 2);
    le(central, 0, 2);
    le(central, 0, 4);
    le(central, offset, 4);
    central.push(...name);
  }
  const at = parts.length;
  const out = [...parts, ...central];
  le(out, 0x06054b50, 4);
  le(out, 0, 4);
  le(out, entries.length, 2);
  le(out, entries.length, 2);
  le(out, central.length, 4);
  le(out, at, 4);
  le(out, 0, 2);
  return new Uint8Array(out);
}

test("a stored and a deflated entry come out as they went in; a missing one or a file that is no ZIP is null", async () => {
  const zip = await zipOf([
    { name: "a.txt", text: "stored text", deflate: false },
    { name: "project.json", text: JSON.stringify({ pcbs: { b: "Board B", a: "Board A" } }), deflate: true },
  ]);
  assert.equal(new TextDecoder().decode((await readZipEntry(zip, "a.txt"))!), "stored text");
  assert.deepEqual(JSON.parse(new TextDecoder().decode((await readZipEntry(zip, "project.json"))!)), { pcbs: { b: "Board B", a: "Board A" } });
  assert.equal(await readZipEntry(zip, "missing.json"), null);
  assert.equal(await readZipEntry(new TextEncoder().encode("not a zip at all, just text that is long enough to be scanned"), "a.txt"), null);
  let binary = "";
  for (const b of zip) binary += String.fromCharCode(b);
  assert.deepEqual(await easyEdaProject(btoa(binary)), { pcbs: { b: "Board B", a: "Board A" } });
  assert.equal(await easyEdaProject(btoa("plain")), null);
});
