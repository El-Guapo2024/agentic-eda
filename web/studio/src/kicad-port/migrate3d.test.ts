import { test } from "node:test";
import assert from "node:assert/strict";
import { defaultChoices, migrateCmds, previewRow, rechoose, replacementsOf, savedSearchDirs, USER_SELECTED_SCORE, userSelectedDisplay, withPickedFile, withSearchDir, type MissingModel } from "./migrate3d";

const cand = (path: string, score = 1000) => ({ path, display: `${path.split("/").pop()}   —   dir`, score });
const missing = (name: string, candidates: ReturnType<typeof cand>[], over: Partial<MissingModel> = {}): MissingModel => ({
  name,
  footprints: ["R1", "R2"],
  rep: "R1",
  model: { name, offset: [0.5, 0, 0.25], scale: [1, 1.5, 1], rotate: [0, 0, 90], opacity: 0.5, show: true },
  candidates,
  ...over,
});

const rows = [missing("a.wrl", [cand("${K}/A/a.step"), cand("${K}/B/a.step", 800)]), missing("b.wrl", []), missing("c.wrl", [cand("${K}/C/c.step", 490)])];

test("every row with a candidate starts on its best, a row with none on 'keep existing'", () => {
  assert.deepEqual(defaultChoices(rows), [0, -1, 0]);
  assert.deepEqual(defaultChoices([]), []);
});

test("Replace sends the old name for the new one for each row that chose, in one command; none chosen is no command", () => {
  assert.deepEqual(replacementsOf(rows, [0, -1, 0]), { "a.wrl": "${K}/A/a.step", "c.wrl": "${K}/C/c.step" });
  assert.deepEqual(replacementsOf(rows, [1, -1, -1]), { "a.wrl": "${K}/B/a.step" });
  assert.deepEqual(migrateCmds(rows, [0, -1, 0]), [{ op: "migrate_board_models", replacements: { "a.wrl": "${K}/A/a.step", "c.wrl": "${K}/C/c.step" } }]);
  assert.deepEqual(migrateCmds(rows, [-1, -1, -1]), [], "Replace without choosing anything is Keep");
  assert.deepEqual(migrateCmds(rows, [9, -1, 7]), [], "a choice the row has no candidate for is none");
});

test("a file picked by hand is the first candidate, chosen, with a score above every ranked one; one the row has is chosen where it is", () => {
  const picked = withPickedFile(rows[0]!, "/x/mine.step", userSelectedDisplay("/x/mine.step"));
  assert.equal(picked.choice, 0);
  assert.deepEqual(picked.model.candidates.map((c) => c.path), ["/x/mine.step", "${K}/A/a.step", "${K}/B/a.step"]);
  assert.equal(picked.model.candidates[0]!.score, USER_SELECTED_SCORE);
  assert.equal(picked.model.candidates[0]!.display, "mine.step   —   user-selected");
  const again = withPickedFile(rows[0]!, "${K}/B/a.step", "ignored");
  assert.deepEqual([again.choice, again.model === rows[0]], [1, true], "no duplicate, the row is the same");
  assert.equal(rows[0]!.candidates.length, 2, "the row it came from is not changed");
});

test("after a folder is added each row keeps its choice when the file is still there, and takes the new best when it is not or it had none", () => {
  const after = [
    missing("a.wrl", [cand("${N}/new/a.step", 1000), cand("${K}/A/a.step", 1000), cand("${K}/B/a.step", 800)]),
    missing("b.wrl", [cand("${N}/new/b.step", 1000)]),
    missing("c.wrl", []),
  ];
  // a.wrl had chosen its second candidate; b.wrl had none; c.wrl had its candidate
  assert.deepEqual(rechoose(rows, [1, -1, 0], after), [2, 0, -1]);
  // a.wrl had its first: that file is now the second
  assert.deepEqual(rechoose(rows, [0, -1, 0], after), [1, 0, -1]);
  // a row the first list did not have takes its best
  assert.deepEqual(rechoose([rows[1]!], [-1], [missing("z.wrl", [cand("q.step")])]), [0]);
});

test("the preview of a candidate is the model as the first footprint has it, in the candidate's file, shown", () => {
  const row = previewRow(rows[0]!, rows[0]!.candidates[1])!;
  assert.deepEqual(row, { path: "${K}/B/a.step", offset: [0.5, 0, 0.25], scale: [1, 1.5, 1], rotate: [0, 0, 90], opacity: 0.5, show: true });
  assert.equal(previewRow(rows[0]!, undefined), null, "keep existing: no model to show");
  row.offset[0] = 9;
  assert.equal(rows[0]!.model.offset[0], 0.5, "a copy");
});

test("a search folder is kept once, and the saved list is empty when the browser keeps none", () => {
  assert.deepEqual(withSearchDir(["a"], "b"), ["a", "b"]);
  assert.deepEqual(withSearchDir(["a", "b"], "a"), ["a", "b"]);
  assert.deepEqual(typeof localStorage === "undefined" ? [] : savedSearchDirs(), []);
});
