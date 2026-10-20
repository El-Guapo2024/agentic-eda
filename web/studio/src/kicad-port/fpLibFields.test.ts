import { test } from "node:test";
import assert from "node:assert/strict";
import { defaultReferenceLayout, gridProblem, moveRow, newRowName, panelOfRows, rowsOfFootprint, type LibFieldRow } from "./fpLibFields";
import type { LibraryFootprint } from "../api/types";

const fp = (over: Partial<LibraryFootprint> = {}) => ({ name: "R_0603", reference_visible: true, value_visible: true, fields: [], ...over }) as unknown as LibraryFootprint;
const fmt = (um: number) => `${um / 1000} mm`;

test("a footprint with nothing set has the Reference REF** and the Value named after it, at KiCad's places", () => {
  const rows = rowsOfFootprint(fp());
  assert.deepEqual(rows.map((r) => [r.name, r.text, r.layout.layer, r.layout.visible]), [["Reference", "REF**", "F.SilkS", true], ["Value", "R_0603", "F.Fab", true]]);
  assert.deepEqual(rows[0]!.layout.at, { x: 0, y: -2000 });
});

test("the layouts and texts a file gave are the rows', and the Show flags are the footprint's", () => {
  const rows = rowsOfFootprint(
    fp({
      reference: { at: { x: 500, y: -2500 }, size: [1200, 1000], thickness: 180, layer: "F.SilkS", text: "J**" },
      value_visible: false,
      fields: [{ name: "MPN", value: "ABC-1", visible: true, layout: { at: { x: 1500, y: 3000 }, size: [800, 800], layer: "F.Fab", bold: true } }, { name: "Vendor", value: "ACME", visible: false }],
    })
  );
  assert.deepEqual([rows[0]!.text, rows[0]!.layout.at, rows[0]!.layout.size], ["J**", { x: 500, y: -2500 }, [1200, 1000]]);
  assert.equal(rows[1]!.layout.visible, false, "the Value is hidden");
  assert.deepEqual([rows[2]!.name, rows[2]!.text, rows[2]!.layout.bold, rows[2]!.layout.visible], ["MPN", "ABC-1", true, true]);
  assert.deepEqual([rows[3]!.name, rows[3]!.layout.visible, rows[3]!.layout.layer, rows[3]!.layout.at], ["Vendor", false, "F.Fab", { x: 0, y: 0 }], "a field never placed is hidden at the origin on the fabrication layer");
});

test("OK sends the text only when it is not the default one, and the Show flags beside the layouts", () => {
  const rows = rowsOfFootprint(fp());
  assert.equal(panelOfRows(rows, "R_0603").reference.text, undefined);
  const edited: LibFieldRow[] = rows.map((r, i) => (i === 0 ? { ...r, text: "R**", layout: { ...r.layout, visible: false } } : r));
  const p = panelOfRows(edited, "R_0603");
  assert.equal(p.reference.text, "R**");
  assert.deepEqual([p.reference_visible, p.value_visible], [false, true]);
  assert.equal(p.value.text, undefined);
  const withField = [...rows, { key: 9, name: " MPN ", text: "x", layout: { ...defaultReferenceLayout(), visible: false } }];
  const q = panelOfRows(withField, "R_0603");
  assert.deepEqual(q.fields.map((f) => [f.name, f.value, f.visible]), [["MPN", "x", false]]);
});

test("the grid's problems are the dialog's: the size, the pen, a name, a mandatory name, a name used twice", () => {
  const rows = rowsOfFootprint(fp({ fields: [{ name: "MPN", value: "a", visible: true }] }));
  assert.equal(gridProblem(rows, fmt), null);
  const tiny = rows.map((r, i) => (i === 0 ? { ...r, layout: { ...r.layout, size: [0, 1000] as [number, number] } } : r));
  assert.match(gridProblem(tiny, fmt)!, /^Reference: Text width must be at least/);
  assert.match(gridProblem(rows.map((r, i) => (i === 2 ? { ...r, name: "" } : r)), fmt)!, /Fields must have a name\./);
  assert.match(gridProblem(rows.map((r, i) => (i === 2 ? { ...r, name: "Datasheet" } : r)), fmt)!, /is the name of a mandatory field/);
  assert.match(gridProblem(rows.map((r, i) => (i === 2 ? { ...r, name: "ki_keywords" } : r)), fmt)!, /not a field/);
  assert.match(gridProblem([...rows, { key: 7, name: "mpn", text: "b", layout: rows[2]!.layout }], fmt)!, /There are two fields called "mpn"\./);
  assert.match(gridProblem(rows.map((r, i) => (i === 2 ? { ...r, name: " MPN" } : r)), fmt)!, /starts or ends with a space/);
  assert.match(gridProblem(rows.map((r, i) => (i === 2 ? { ...r, layout: { ...r.layout, thickness: 900 } } : r)), fmt)!, /thickness is too large/);
});

test("a new field is Field<rows>, or the next number free", () => {
  const rows = rowsOfFootprint(fp());
  assert.equal(newRowName(rows), "Field2");
  assert.equal(newRowName([...rows, { key: 5, name: "Field3", text: "", layout: rows[0]!.layout }]), "Field4", "Field2 is free but the rows count 3 now");
});

test("rows move one place, never into the two mandatory rows", () => {
  const rows = ["R", "V", "a", "b", "c"].map((name, key) => ({ key, name }));
  assert.deepEqual(moveRow(rows, 2, -1), rows, "the first user row cannot go up into the Value");
  assert.deepEqual(moveRow(rows, 1, 1), rows, "a mandatory row does not move");
  assert.deepEqual(moveRow(rows, 4, 1), rows, "the last row cannot go down");
  assert.deepEqual(moveRow(rows, 3, -1).map((r) => r.name), ["R", "V", "b", "a", "c"]);
  assert.deepEqual(moveRow(rows, 2, 1).map((r) => r.name), ["R", "V", "b", "a", "c"]);
});
