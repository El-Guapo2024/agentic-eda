// Tests for tools/extract-erc-checks.js: how it reads KiCad's ERC item list (eeschema/erc/erc_item.cpp) and default severities (erc_settings.cpp), and that
// the two files it writes -- src/kicad/erc_checks.json for the Violation Severity page, crates/model/src/erc_checks.rs for the verb that stores the
// table -- say the same thing. Plain Node: node --test tools/lib/ercChecks.test.js
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { build, parseItems, parseOrder, parseSeverities } from "../extract-erc-checks.js";

const here = dirname(fileURLToPath(import.meta.url));
const read = (rel) => readFileSync(join(here, "..", "..", rel), "utf8");

const ITEMS = `
ERC_ITEM ERC_ITEM::heading_connections( 0, _HKI( "Connections" ), "" );
ERC_ITEM ERC_ITEM::heading_conflicts( 0, _HKI( "Conflicts" ), "" );
ERC_ITEM ERC_ITEM::heading_internal( 0, "", "" );

ERC_ITEM ERC_ITEM::pinNotConnected( ERCE_PIN_NOT_CONNECTED,
        _HKI( "Pin not connected" ),
        wxT( "pin_not_connected" ) );

ERC_ITEM ERC_ITEM::noConnectConnected( ERCE_NOCONNECT_CONNECTED,
        _HKI( "A pin with a \\"no connection\\" flag is connected" ),
        wxT( "no_connect_connected" ) );

ERC_ITEM ERC_ITEM::pinTableWarning( ERCE_PIN_TO_PIN_WARNING,
        _HKI( "Conflict problem between pins" ),
        wxT( "pin_to_pin" ) );

ERC_ITEM ERC_ITEM::genericError( ERCE_GENERIC_ERROR,
        _HKI( "Error" ),
        wxT( "generic-error" ) );

std::vector<std::reference_wrapper<RC_ITEM>> ERC_ITEM::allItemTypes(
        {
                ERC_ITEM::heading_connections,
                ERC_ITEM::pinNotConnected,
                ERC_ITEM::noConnectConnected,
                // Commented out until the logic for this element is coded
                //                 ERC_ITEM::busLabelSyntax,

                ERC_ITEM::heading_conflicts,
                ERC_ITEM::pinTableWarning,

                ERC_ITEM::heading_internal,
                ERC_ITEM::pinTableWarning,
                ERC_ITEM::genericError
        } );
`;

const SETTINGS = `
    for( int i = ERCE_FIRST; i <= ERCE_LAST; ++i )
        m_ERCSeverities[ i ] = RPT_SEVERITY_ERROR;

    m_ERCSeverities[ERCE_UNSPECIFIED]             = RPT_SEVERITY_UNDEFINED;
    m_ERCSeverities[ERCE_PIN_TO_PIN_WARNING]      = RPT_SEVERITY_WARNING;
    m_ERCSeverities[ERCE_NOCONNECT_CONNECTED]     = RPT_SEVERITY_WARNING;
    m_ERCSeverities[ERCE_SINGLE_GLOBAL_LABEL] = RPT_SEVERITY_IGNORE;
`;

test("items are read with their code, title (escaped quotes undone) and settings key", () => {
  const items = parseItems(ITEMS);
  assert.deepEqual(items.get("pinNotConnected"), { code: "ERCE_PIN_NOT_CONNECTED", title: "Pin not connected", key: "pin_not_connected" });
  assert.equal(items.get("noConnectConnected").title, 'A pin with a "no connection" flag is connected');
  assert.equal(items.get("genericError").key, "generic-error");
});

test("the order is the list's, with commented-out entries left out", () => {
  const order = parseOrder(ITEMS);
  assert.ok(!order.includes("busLabelSyntax"), "a commented-out item is not an item");
  assert.deepEqual(order.slice(0, 3), ["heading_connections", "pinNotConnected", "noConnectConnected"]);
});

test("a severity is the constructor's assignment, else Error", () => {
  const sev = parseSeverities(SETTINGS);
  assert.equal(sev.get("ERCE_NOCONNECT_CONNECTED"), "warning");
  assert.equal(sev.get("ERCE_SINGLE_GLOBAL_LABEL"), "ignore");
  assert.equal(sev.has("ERCE_UNSPECIFIED"), false, "undefined is no severity");
});

test("the groups stop at the internal heading and the pin conflicts map row is taken out of its group, as PANEL_SETUP_SEVERITIES does", () => {
  const { groups, pinMap } = build(ITEMS, SETTINGS);
  assert.deepEqual(
    groups.map((g) => [g.title, g.items.map((i) => i.key)]),
    [["Connections", ["pin_not_connected", "no_connect_connected"]]],
    "the Conflicts heading is left with nothing (the pin map row), so it is not a group; the internal ones are not listed"
  );
  assert.equal(groups[0].items[0].defaultSeverity, "error");
  assert.equal(groups[0].items[1].defaultSeverity, "warning");
  assert.deepEqual(pinMap, { key: "pin_to_pin", title: "Conflict problem between pins", code: "ERCE_PIN_TO_PIN_WARNING", defaultSeverity: "warning" });
});

test("the JSON the page reads and the list the backend accepts are the same checks with the same defaults", () => {
  const json = JSON.parse(read("src/kicad/erc_checks.json"));
  const fromJson = [...json.groups.flatMap((g) => g.items), json.pinMap].map((i) => [i.key, i.defaultSeverity]);
  const rs = read("../../crates/model/src/erc_checks.rs");
  const body = rs.slice(rs.indexOf("pub const ERC_CHECKS"), rs.indexOf("];", rs.indexOf("pub const ERC_CHECKS")));
  const fromRust = [...body.matchAll(/\("([^"]+)", "([^"]+)"\)/g)].map((m) => [m[1], m[2]]);
  assert.deepEqual(fromRust, fromJson);
  assert.equal(new Set(fromJson.map(([k]) => k)).size, fromJson.length, "every key once");
  for (const [key, severity] of fromJson) assert.ok(["error", "warning", "ignore"].includes(severity), `${key}: ${severity}`);
  assert.ok(fromJson.length > 40, "the whole list, not a fragment");
});
