import { test } from "node:test";
import assert from "node:assert/strict";
import {
  COMMENT_COUNT,
  DEFAULT_PAPER,
  MAX_PAGE_SIZE_PCBNEW_NM,
  PAGE_FORMATS,
  USER_PAPER,
  isDefaultPaper,
  isoDate,
  normalizePaper,
  paperLabel,
  paperOf,
  paperSizeNm,
  paperToCmd,
  samePaper,
  shownTitleBlock,
  titleBlockFields,
  titleBlockToSend,
  userSizeError,
} from "./pageSettings";

test("every standard format is landscape, like PAGE_INFO::standardPageSizes", () => {
  for (const f of PAGE_FORMATS) assert.ok(f.width > f.height, f.name);
});

test("a standard paper has the size of its format, turned over for portrait", () => {
  assert.deepEqual(paperSizeNm(DEFAULT_PAPER), { width: 297_000_000, height: 210_000_000 });
  assert.deepEqual(paperSizeNm({ paper: "A4", portrait: true, userSize: null }), { width: 210_000_000, height: 297_000_000 });
  assert.deepEqual(paperSizeNm({ paper: "USLetter", portrait: false, userSize: null }), { width: 279_400_000, height: 215_900_000 });
  assert.deepEqual(paperSizeNm({ paper: "nonsense", portrait: false, userSize: null }), { width: 297_000_000, height: 210_000_000 }, "an unknown name is A4");
});

test("a user paper is its own width and height, whatever the orientation flag says", () => {
  assert.deepEqual(paperSizeNm({ paper: USER_PAPER, portrait: true, userSize: [100_000_000, 300_000_000] }), { width: 100_000_000, height: 300_000_000 });
});

test("the state JSON's page is read back (nothing is the default)", () => {
  assert.deepEqual(paperOf(null), DEFAULT_PAPER);
  assert.deepEqual(paperOf(undefined), DEFAULT_PAPER);
  assert.deepEqual(paperOf({ paper: "A3", portrait: true, user_size_nm: null }), { paper: "A3", portrait: true, userSize: null });
  assert.deepEqual(paperOf({ paper: "User", user_size_nm: [300_000_000, 200_000_000] }), { paper: "User", portrait: false, userSize: [300_000_000, 200_000_000] });
});

test("normalising drops what does not apply and gives a new user paper KiCad's 17 x 11 in", () => {
  assert.deepEqual(normalizePaper({ paper: "A4", portrait: true, userSize: [1, 2000] }), { paper: "A4", portrait: true, userSize: null });
  assert.deepEqual(normalizePaper({ paper: USER_PAPER, portrait: true, userSize: [300_000_000, 200_000_000] }), { paper: USER_PAPER, portrait: false, userSize: [300_000_000, 200_000_000] });
  assert.deepEqual(normalizePaper({ paper: USER_PAPER, portrait: false, userSize: null }), { paper: USER_PAPER, portrait: false, userSize: [431_800_000, 279_400_000] });
});

test("the default is A4 landscape and nothing else", () => {
  assert.equal(isDefaultPaper(DEFAULT_PAPER), true);
  assert.equal(isDefaultPaper({ paper: "A4", portrait: true, userSize: null }), false);
  assert.equal(isDefaultPaper({ paper: "A3", portrait: false, userSize: null }), false);
  assert.equal(isDefaultPaper({ paper: USER_PAPER, portrait: false, userSize: [297_000_000, 210_000_000] }), false);
});

test("two papers are the same page when they normalise alike", () => {
  assert.equal(samePaper(DEFAULT_PAPER, { paper: "A4", portrait: false, userSize: [1, 1] }), true);
  assert.equal(samePaper(DEFAULT_PAPER, { paper: "A4", portrait: true, userSize: null }), false);
  assert.equal(samePaper({ paper: USER_PAPER, portrait: false, userSize: [1, 2000] }, { paper: USER_PAPER, portrait: true, userSize: [1, 2000] }), true);
});

test("a user size must be inside the editor's limits", () => {
  assert.equal(userSizeError(300_000_000, 200_000_000, MAX_PAGE_SIZE_PCBNEW_NM), null);
  assert.match(userSizeError(10_000_000, 200_000_000, MAX_PAGE_SIZE_PCBNEW_NM)!, /width must be between 25\.4 mm and 1219\.2 mm/);
  assert.match(userSizeError(300_000_000, 2_000_000_000, MAX_PAGE_SIZE_PCBNEW_NM)!, /height must be between/);
  assert.match(userSizeError(Number.NaN, 200_000_000, MAX_PAGE_SIZE_PCBNEW_NM)!, /must be numbers/);
});

test("the dialog's labels are KiCad's descriptions", () => {
  assert.equal(paperLabel("A4"), "A4 210 x 297mm");
  assert.equal(paperLabel("USLedger"), "US Ledger 11 x 17in");
  assert.equal(paperLabel(USER_PAPER), "User (Custom)");
});

test("the title block always has nine comments to edit, and trailing empty ones are not sent", () => {
  const f = titleBlockFields({ title: "T", comments: ["one", "", "three"] });
  assert.equal(f.comments.length, COMMENT_COUNT);
  assert.equal(f.comments[2], "three");
  assert.equal(titleBlockFields(null).comments.every((c) => c === ""), true);
  assert.deepEqual(titleBlockToSend(f), { title: "T", date: "", rev: "", company: "", comments: ["one", "", "three"] });
  assert.deepEqual(titleBlockToSend(titleBlockFields(null)).comments, []);
});

test("the paper is sent as the verb takes it", () => {
  assert.deepEqual(paperToCmd(DEFAULT_PAPER), { paper: "A4" });
  assert.deepEqual(paperToCmd({ paper: "A3", portrait: true, userSize: null }), { paper: "A3", portrait: true });
  assert.deepEqual(paperToCmd({ paper: USER_PAPER, portrait: true, userSize: [300_000_000, 200_000_000] }), { paper: USER_PAPER, user_size_nm: [300_000_000, 200_000_000] });
});

test("the date button writes the ISO form", () => {
  assert.equal(isoDate(new Date(2026, 9, 7)), "2026-10-07");
  assert.equal(isoDate(new Date(2026, 0, 3)), "2026-01-03");
});

test("the drawing sheet shows the title block with the project's variables expanded and the dialog keeps the stored values", () => {
  const stored = { title: "Board ${REV}", date: "", rev: "C", company: "", comments: [] };
  assert.equal(shownTitleBlock(stored), stored, "nothing to expand: the block itself");
  assert.equal(shownTitleBlock(null), null);
  const withShown = { ...stored, shown: { title: "Board B", date: "", rev: "C", company: "", comments: [] } };
  assert.deepEqual(shownTitleBlock(withShown), { title: "Board B", date: "", rev: "C", company: "", comments: [], shown: undefined });
  assert.equal(withShown.title, "Board ${REV}", "the stored block is not changed");
});
