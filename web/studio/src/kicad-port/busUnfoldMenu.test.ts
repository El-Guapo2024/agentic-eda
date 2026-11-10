import { test } from "node:test";
import assert from "node:assert/strict";
import { busUnfoldMenu, printBusForUI } from "./busUnfoldMenu";

test("PrintBusForUI drops the markup's braces and shows an escaped space", () => {
  assert.equal(printBusForUI("D[0..7]"), "D[0..7]");
  assert.equal(printBusForUI("~{RESET}"), "~RESET");
  assert.equal(printBusForUI("USB{DP DN}"), "USB{DP DN");
  assert.equal(printBusForUI("A\\ B"), "A B");
});

test("a vector's members are plain items and a bus in a group is a submenu", () => {
  const menu = busUnfoldMenu([
    { name: "SCL", members: [] },
    { name: "B[0..1]", members: ["B0", "B1"] },
  ]);
  assert.deepEqual(menu, [
    { kind: "item", name: "SCL", label: "SCL" },
    { kind: "menu", title: "B[0..1]", label: "B[0..1]", entries: [{ kind: "item", name: "B0", label: "B0" }, { kind: "item", name: "B1", label: "B1" }] },
  ]);
});

test("the members of a differential pair are gathered in a submenu named without the suffix", () => {
  // `{USB CLK}` where the alias USB is {DP DN} and CLK a net: USB's members end in P and N, so they go in `USB.D`'s submenu
  const menu = busUnfoldMenu([
    { name: "USB", members: ["USB.DP", "USB.DN", "USB.ID"] },
    { name: "CLK", members: [] },
  ]);
  assert.equal(menu.length, 2);
  const usb = menu[0]!;
  assert.ok(usb.kind === "menu" && usb.title === "USB");
  if (usb.kind !== "menu") return;
  assert.deepEqual(
    usb.entries.map((e) => (e.kind === "menu" ? [e.title, e.entries.map((x) => (x.kind === "item" ? x.name : ""))] : e.name)),
    [["USB.D", ["USB.DP", "USB.DN"]], "USB.ID"]
  );
  // `+` and `-` pair the same way
  const lvds = busUnfoldMenu([{ name: "L", members: ["TX+", "TX-"] }]);
  assert.ok(lvds[0]!.kind === "menu" && lvds[0]!.entries.length === 1 && lvds[0]!.entries[0]!.kind === "menu" && lvds[0]!.entries[0]!.title === "TX");
});

test("a suffixed bus member shares one submenu by its name, and a pair met again lands where it was made", () => {
  // two vector members named D+ and D- (each with members): one submenu `D` at the top for both
  const menu = busUnfoldMenu([
    { name: "D+", members: ["A0", "A1"] },
    { name: "D-", members: ["B0"] },
  ]);
  assert.equal(menu.length, 1);
  assert.ok(menu[0]!.kind === "menu" && menu[0]!.title === "D");
  if (menu[0]!.kind !== "menu") return;
  assert.deepEqual(
    menu[0]!.entries.map((e) => (e.kind === "item" ? e.name : e.title)),
    ["A0", "A1", "B0"]
  );
  // the map is the whole menu's: the pair X+ / X- under a second member goes into the submenu the first member made
  const shared = busUnfoldMenu([
    { name: "G1", members: ["X+"] },
    { name: "G2", members: ["X-"] },
  ]);
  const g1 = shared[0]!;
  const g2 = shared[1]!;
  assert.ok(g1.kind === "menu" && g2.kind === "menu");
  if (g1.kind !== "menu" || g2.kind !== "menu") return;
  assert.equal(g2.entries.length, 0, "G2's submenu stays empty");
  assert.ok(g1.entries[0]!.kind === "menu" && g1.entries[0]!.entries.length === 2);
});
