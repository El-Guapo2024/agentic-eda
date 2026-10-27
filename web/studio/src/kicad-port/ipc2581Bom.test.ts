import { test } from "node:test";
import assert from "node:assert/strict";
import { distributorAfter, guessDistributor, ipc2581Chosen, ipc2581DialogStart, ipc2581FieldOptions, manufacturerAfter, NOT_APPLICABLE } from "./ipc2581Bom";

test("the column choices are the footprints' field names, the mandatory four included, sorted once each", () => {
  const opts = ipc2581FieldOptions([{ fields: [{ name: "Reference" }, { name: "LCSC" }, { name: "MPN" }] }, { fields: [{ name: "MPN" }, { name: "Manufacturer" }] }]);
  assert.deepEqual(opts, ["Datasheet", "Description", "LCSC", "MPN", "Manufacturer", "Reference", "Value"]);
  assert.deepEqual(ipc2581FieldOptions([]), [], "no footprint, no field");
});

test("the distributor is guessed from the Distributor P/N column's name (onDistPNChange)", () => {
  assert.equal(guessDistributor("LCSC"), "LCSC");
  assert.equal(guessDistributor("DigiKey PN"), "Digi-Key");
  assert.equal(guessDistributor("Digi-Key_PN"), "Digi-Key");
  assert.equal(guessDistributor("mouser#"), "Mouser");
  assert.equal(guessDistributor("Supplier"), null);
  assert.equal(distributorAfter("LCSC", NOT_APPLICABLE), "LCSC");
  assert.equal(distributorAfter("LCSC", "Acme"), "Acme", "one already typed is kept");
  assert.equal(distributorAfter("Supplier", NOT_APPLICABLE), NOT_APPLICABLE);
  assert.equal(distributorAfter("", "LCSC"), NOT_APPLICABLE, "Omit disables it");
});

test("a Manufacturer P/N column picks a manufacturer column, case ignored (onMfgPNChange)", () => {
  assert.equal(manufacturerAfter("MPN", "", ["MPN", "MANUFACTURER", "Mfg"]), "MANUFACTURER");
  assert.equal(manufacturerAfter("MPN", "", ["MPN", "Mfg"]), "Mfg");
  assert.equal(manufacturerAfter("MPN", "Value", ["MPN", "Mfg"]), "Value");
  assert.equal(manufacturerAfter("", "", ["Mfg"]), "");
});

test("the dialog starts from the kept columns that still exist (TransferDataToWindow)", () => {
  const opts = ["Datasheet", "Description", "LCSC", "MPN", "Mfg", "Reference", "Value"];
  assert.deepEqual(ipc2581DialogStart(undefined, "B", opts), { bom_rev: "B", col_id: "", col_mpn: "", col_mfg: "", col_dist_pn: "", col_dist: NOT_APPLICABLE });
  const kept = { id: "Reference", mpn: "MPN", mfg: "Mfg", dist_pn: "LCSC", dist: "LCSC Electronics", bom_rev: "3" };
  assert.deepEqual(ipc2581DialogStart(kept, "B", opts), { bom_rev: "3", col_id: "Reference", col_mpn: "MPN", col_mfg: "Mfg", col_dist_pn: "LCSC", col_dist: "LCSC Electronics" });
  assert.deepEqual(ipc2581DialogStart({ mpn: "Gone", mfg: "Mfg", dist_pn: "Gone" }, "", opts), { bom_rev: "", col_id: "", col_mpn: "", col_mfg: "", col_dist_pn: "", col_dist: NOT_APPLICABLE });
});

// X5-2: 25ec2335's DIALOG_EXPORT_2581_BOM::TransferDataToWindow reads the Internal ID back with the other columns.
test("the internal id is read back as 25ec2335's BOM dialog does", () => {
  assert.equal(ipc2581DialogStart({ id: "Reference" }, "", ["Reference", "Value"]).col_id, "Reference");
  assert.equal(ipc2581DialogStart({ id: "Gone" }, "", ["Reference", "Value"]).col_id, "", "a column the board has lost is Generate unique again");
});

test("what Export keeps and passes: Manufacturer and Distributor only with their P/N column, and never N/A", () => {
  assert.deepEqual(ipc2581Chosen({ bom_rev: "A", col_id: "Reference", col_mpn: "", col_mfg: "Mfg", col_dist_pn: "LCSC", col_dist: "LCSC" }), { id: "Reference", mpn: "", mfg: "", dist_pn: "LCSC", dist: "LCSC", bom_rev: "A" });
  assert.equal(ipc2581Chosen({ bom_rev: "", col_id: "", col_mpn: "", col_mfg: "", col_dist_pn: "Supplier", col_dist: NOT_APPLICABLE }).dist, "");
});
