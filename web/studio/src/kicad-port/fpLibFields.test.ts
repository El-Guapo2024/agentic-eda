import { test } from "node:test";
import assert from "node:assert/strict";
import { checkLibFieldRow, defaultReferenceLayout, gridProblem, moveRow, newRowName, panelOfRows, pastedFootprint, rowsOfFootprint, type LibFieldRow } from "./fpLibFields";
import type { LibraryFootprint } from "../api/types";

const fp = (over: Partial<LibraryFootprint> = {}) => ({ name: "R_0603", reference_visible: true, value_visible: true, fields: [], ...over }) as unknown as LibraryFootprint;
const fmt = (nm: number) => `${nm / 1e6} mm`;

test("a footprint with nothing set has the Reference REF** and the Value named after it, at KiCad's places, then the empty hidden Datasheet and Description", () => {
  const rows = rowsOfFootprint(fp());
  // `PCB_FIELDS_GRID_TABLE::GetMandatoryRowCount`: a footprint's four mandatory fields
  assert.deepEqual(rows.map((r) => [r.name, r.text, r.layout.layer, r.layout.visible]), [
    ["Reference", "REF**", "F.SilkS", true],
    ["Value", "R_0603", "F.Fab", true],
    ["Datasheet", "", "F.Fab", false],
    ["Description", "", "F.Fab", false],
  ]);
  // `CreateNewFootprint`: REF** half a 1 mm text height above the origin, in the silkscreen's 0.1 mm pen; the Value at (0, 1 mm).
  assert.deepEqual([rows[0]!.layout.at, rows[0]!.layout.thickness, rows[1]!.layout.at], [{ x: 0, y: -500_000 }, 100_000, { x: 0, y: 1_000_000 }]);
  assert.deepEqual([rows[2]!.layout.size, rows[2]!.layout.thickness], [[1_270_000, 1_270_000], 0], "EDA_TEXT's 1.27 mm with the automatic pen");
});

test("the Datasheet and the Description rows read the footprint's fields and OK sends them with their text", () => {
  const rows = rowsOfFootprint(fp({ datasheet: { at: { x: 1_000_000, y: 1_000_000 }, size: [1_000_000, 1_000_000], thickness: 150_000, layer: "F.Fab", visible: false, text: "https://x/y.pdf" } }));
  assert.deepEqual([rows[2]!.name, rows[2]!.text, rows[2]!.layout.at], ["Datasheet", "https://x/y.pdf", { x: 1_000_000, y: 1_000_000 }]);
  const edited = rows.map((r, i) => (i === 3 ? { ...r, text: "fingers" } : r));
  const p = panelOfRows(edited, "R_0603");
  assert.equal(p.datasheet.text, "https://x/y.pdf");
  assert.deepEqual([p.description_field.text, p.description_field.layer], ["fingers", "F.Fab"]);
  assert.deepEqual(p.fields, [], "the mandatory rows are no user fields");
});

test("the layouts and texts a file gave are the rows', and the Show flags are the footprint's", () => {
  const rows = rowsOfFootprint(
    fp({
      reference: { at: { x: 500_000, y: -2_500_000 }, size: [1_200_000, 1_000_000], thickness: 180_000, layer: "F.SilkS", text: "J**" },
      value_visible: false,
      fields: [{ name: "MPN", value: "ABC-1", visible: true, layout: { at: { x: 1_500_000, y: 3_000_000 }, size: [800_000, 800_000], layer: "F.Fab", bold: true } }, { name: "Vendor", value: "ACME", visible: false }],
    })
  );
  assert.deepEqual([rows[0]!.text, rows[0]!.layout.at, rows[0]!.layout.size], ["J**", { x: 500_000, y: -2_500_000 }, [1_200_000, 1_000_000]]);
  assert.equal(rows[1]!.layout.visible, false, "the Value is hidden");
  assert.deepEqual([rows[4]!.name, rows[4]!.text, rows[4]!.layout.bold, rows[4]!.layout.visible], ["MPN", "ABC-1", true, true]);
  assert.deepEqual([rows[5]!.name, rows[5]!.layout.visible, rows[5]!.layout.layer, rows[5]!.layout.at], ["Vendor", false, "F.Fab", { x: 0, y: 0 }], "a field never placed is hidden at the origin on the fabrication layer");
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
  const tiny = rows.map((r, i) => (i === 0 ? { ...r, layout: { ...r.layout, size: [0, 1_000_000] as [number, number] } } : r));
  assert.match(gridProblem(tiny, fmt)!, /^Reference: The text width must be between/);
  assert.match(gridProblem(rows.map((r, i) => (i === 4 ? { ...r, name: "" } : r)), fmt)!, /Fields must have a name\./);
  assert.match(gridProblem(rows.map((r, i) => (i === 4 ? { ...r, name: "Datasheet" } : r)), fmt)!, /is the name of a mandatory field/);
  assert.match(gridProblem(rows.map((r, i) => (i === 4 ? { ...r, name: "ki_keywords" } : r)), fmt)!, /not a field/);
  assert.match(gridProblem([...rows, { key: 7, name: "mpn", text: "b", layout: rows[4]!.layout }], fmt)!, /There are two fields called "mpn"\./);
  assert.match(gridProblem(rows.map((r, i) => (i === 4 ? { ...r, name: " MPN" } : r)), fmt)!, /starts or ends with a space/);
  assert.match(gridProblem(rows.map((r, i) => (i === 4 ? { ...r, layout: { ...r.layout, thickness: 900_000 } } : r)), fmt)!, /thickness is too large/);
});

test("a new field is Field<rows>, or the next number free", () => {
  const rows = rowsOfFootprint(fp());
  assert.equal(newRowName(rows), "Field4");
  assert.equal(newRowName([...rows, { key: 5, name: "Field5", text: "", layout: rows[0]!.layout }]), "Field6", "Field4 is free but the rows count 5 now");
});

test("rows move one place, never into the four mandatory rows", () => {
  const rows = ["R", "V", "D", "E", "a", "b", "c"].map((name, key) => ({ key, name }));
  assert.deepEqual(moveRow(rows, 4, -1), rows, "the first user row cannot go up into the Description");
  assert.deepEqual(moveRow(rows, 3, 1), rows, "a mandatory row does not move");
  assert.deepEqual(moveRow(rows, 6, 1), rows, "the last row cannot go down");
  assert.deepEqual(moveRow(rows, 5, -1).map((r) => r.name), ["R", "V", "D", "E", "b", "a", "c"]);
  assert.deepEqual(moveRow(rows, 4, 1).map((r) => r.name), ["R", "V", "D", "E", "b", "a", "c"]);
});

test("the library dialog's size messages are the Footprint Editor's (DIALOG_FOOTPRINT_PROPERTIES_FP_EDITOR::Validate)", () => {
  assert.equal(checkLibFieldRow("A", { size: [0, 1_000_000], thickness: 0 }, fmt), "The text width must be between 0.001 mm and 250 mm.");
  assert.equal(checkLibFieldRow("A", { size: [1_000_000, 300_000_000], thickness: 0 }, fmt), "The text height must be between 0.001 mm and 250 mm.");
  assert.equal(checkLibFieldRow("A", { size: [1_000_000, 1_000_000], thickness: 900_000 }, fmt), "The text thickness is too large for the text size.\nIt will be clamped.");
  assert.equal(checkLibFieldRow("", { size: [1_000_000, 1_000_000], thickness: 0 }, fmt), "Fields must have a name.");
  assert.equal(checkLibFieldRow("A", { size: [1_000_000, 1_000_000], thickness: 150_000 }, fmt), null);
});

test("Paste Footprint renames the copy and keeps its Value (FOOTPRINT_EDITOR_CONTROL::PasteFootprint only does SetFPID)", () => {
  const plain = pastedFootprint(fp({ name: "X" }), "X_copy");
  assert.equal(plain.name, "X_copy");
  assert.equal(plain.value?.text, "X");
  assert.equal(rowsOfFootprint(plain)[1]!.text, "X", "the Value shows the old name");
  const own = pastedFootprint(fp({ name: "X", value: { at: { x: 0, y: 2 }, size: [1, 1], layer: "F.Fab", text: "10k" } }), "X_copy");
  assert.deepEqual([own.value?.text, own.value?.at], ["10k", { x: 0, y: 2 }]);
});
