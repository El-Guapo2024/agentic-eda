// Tests for tools/extract-sim-models.js: the C++ it runs to copy KiCad's simulation model tables -- one `PARAM::INFO` mutated and copied at
// each push_back, positional constructor arguments, the static tables, switch, strcmp, range-for over an enum -- and, when KICAD_SRC_DIR points
// at KiCad's source, that src/kicad/sim_models.json is what the extractor makes of it (`--check`). Plain Node: node --test tools/lib/simModels.test.js
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { findFunctions, Interpreter, readEnums, readInfoCtor, readNgspiceData, tokenize } from "../extract-sim-models.js";

const here = dirname(fileURLToPath(import.meta.url));

const HEADER = `
class SIM_MODEL {
public:
    DEFINE_ENUM_CLASS_WITH_ITERATOR( TYPE, NONE, V, I, R )
    struct PARAM {
        enum DIR { DIR_IN, DIR_OUT, DIR_INOUT };
        struct INFO
        {
            INFO( const std::string& aName = "",
                  unsigned aId = 0,
                  DIR aDir = DIR_INOUT,
                  SIM_VALUE::TYPE aType = SIM_VALUE::TYPE_FLOAT,
                  const std::string& aUnit = "",
                  CATEGORY aCategory = CATEGORY::PRINCIPAL,
                  const std::string& aDefaultValue = "",
                  const std::string& legacy_unused_value = "",
                  const std::string& aDescription = "",
                  bool aIsSpiceInstanceParam = false,
                  std::vector<std::string> aEnumValues = {} ) :
                name( aName ), id( aId ), dir( aDir ), type( aType ), unit( aUnit ), category( aCategory ),
                defaultValue( aDefaultValue ), description( aDescription ), isSpiceInstanceParam( aIsSpiceInstanceParam ),
                enumValues( std::move( aEnumValues ) )
            {}
        };
    };
};
`;

const SOURCE = `
namespace
{
    // a helper of the file
    bool isWordChar( char c ) { return c == '_'; }
}

SIM_MODEL_SOURCE::SIM_MODEL_SOURCE( TYPE aType ) :
        SIM_MODEL( aType, std::make_unique<SPICE_GENERATOR_SOURCE>( *this ) )
{
    for( const SIM_MODEL::PARAM::INFO& paramInfo : makeParamInfos( aType ) )
        AddParam( paramInfo );
}

const std::vector<SIM_MODEL::PARAM::INFO>& SIM_MODEL_SOURCE::makeParamInfos( TYPE aType )
{
    static std::vector<PARAM::INFO> vdc = makeDcParamInfos( "y", "V" );
    static std::vector<PARAM::INFO> idc = makeDcParamInfos( "y", "A" );

    switch( aType )
    {
    case TYPE::V:              return vdc;
    case TYPE::I:              return idc;
    default:
        wxFAIL_MSG( "Unhandled SIM_MODEL type in SIM_MODEL_SOURCE" );
        static std::vector<SIM_MODEL::PARAM::INFO> empty;
        return empty;
    }
}

std::vector<SIM_MODEL::PARAM::INFO> SIM_MODEL_SOURCE::makeDcParamInfos( const std::string& aPrefix, const std::string& aUnit )
{
    std::vector<PARAM::INFO> paramInfos;
    PARAM::INFO paramInfo;

    paramInfo.name = aPrefix + "1";
    paramInfo.id = 7;
    paramInfo.unit = aUnit;
    paramInfo.description = aUnit == "V" ? "Voltage" : "Current";
    paramInfos.push_back( paramInfo );

    paramInfo.name = "dc";
    paramInfo.enumValues = { "a", "b" };
    paramInfos.push_back( paramInfo );

    appendSp( paramInfos, aUnit );
    return paramInfos;
}

void SIM_MODEL_SOURCE::appendSp( std::vector<PARAM::INFO>& aParamInfos, const std::string& aUnit )
{
    PARAM::INFO paramInfo;

    if( !strcmp( aUnit.c_str(), "V" ) )
    {
        paramInfo.name = "portnum";
        paramInfo.type = SIM_VALUE::TYPE_INT;
        aParamInfos.push_back( paramInfo );
    }
}

SIM_MODEL_RAW::SIM_MODEL_RAW( const std::string& aSpiceSource ) :
    SIM_MODEL( TYPE::RAWSPICE )
{
    static std::vector<PARAM::INFO> paramInfos = makeParamInfos();
    for( const PARAM::INFO& paramInfo : paramInfos )
        AddParam( paramInfo );
    SetParamValue( "model", aSpiceSource );
}

std::vector<SIM_MODEL::PARAM::INFO> SIM_MODEL_RAW::makeParamInfos()
{
    std::vector<PARAM::INFO> paramInfos;

    for( TYPE t : TYPE_ITERATOR() )
    {
        PARAM::INFO paramInfo( "x", 1, PARAM::DIR_OUT );

        switch( t )
        {
        case TYPE::V:
            paramInfo.name = "type";
            paramInfos.push_back( paramInfo );
            break;
        case TYPE::R:
        case TYPE::I:
            paramInfo.name = "lib";
            paramInfos.push_back( paramInfo );
            break;
        default:
            break;
        }
    }

    return paramInfos;
}
`;

function interp() {
  const head = tokenize(HEADER);
  const fns = findFunctions(tokenize(SOURCE));
  return new Interpreter(fns, readEnums(head), readInfoCtor(head));
}

test("comments, preprocessor lines and adjacent strings are tokens as a compiler sees them", () => {
  const t = tokenize('#include <x.h>\n// "no"\nf( "a" "b", /* c */ 1e-3, \'q\' ); a::b');
  assert.deepEqual(
    t.map((x) => x.v),
    ["f", "(", "ab", ",", "1e-3", ",", "q", ")", ";", "a", "::", "b"]
  );
});

test("PARAM::INFO's constructor: each argument's member and default, the unused legacy one setting nothing", () => {
  const ctor = readInfoCtor(tokenize(HEADER));
  assert.deepEqual(
    ctor.params.map((p) => [p.name, p.member]),
    [
      ["aName", "name"],
      ["aId", "id"],
      ["aDir", "dir"],
      ["aType", "type"],
      ["aUnit", "unit"],
      ["aCategory", "category"],
      ["aDefaultValue", "defaultValue"],
      ["legacy_unused_value", null],
      ["aDescription", "description"],
      ["aIsSpiceInstanceParam", "isSpiceInstanceParam"],
      ["aEnumValues", "enumValues"],
    ]
  );
  assert.equal(ctor.params[2].def.name, "DIR_INOUT");
  assert.deepEqual(ctor.params[10].def, []);
});

test("one PARAM::INFO mutated and copied at each push_back (a field a block does not set keeps the block before's), statics, switch on an enum, strcmp, a ternary", () => {
  const it = interp();
  const enumOf = (n) => {
    // an Enum as the interpreter makes them: evaluate `TYPE::n`
    const toks = tokenize(`TYPE::${n}`);
    return it.evalRange(toks, 0, toks.length, { vars: new Map(), def: { name: "t" }, params: [] });
  };
  const v = it.call("SIM_MODEL_SOURCE::SIM_MODEL_SOURCE", [enumOf("V")]);
  assert.deepEqual(
    v.map((p) => [p.name, p.id, p.unit, p.description, p.enumValues, p.type.name]),
    [
      ["y1", 7, "V", "Voltage", [], "TYPE_FLOAT"],
      ["dc", 7, "V", "Voltage", ["a", "b"], "TYPE_FLOAT"],
      ["portnum", 0, "", "", [], "TYPE_INT"],
    ]
  );
  const i = it.call("SIM_MODEL_SOURCE::SIM_MODEL_SOURCE", [enumOf("I")]);
  assert.deepEqual(i.map((p) => [p.name, p.description]), [["y1", "Current"], ["dc", "Current"]], "no S-parameter port for a current source");
});

test("a range-for over an enum's iterator runs each case with a fresh PARAM::INFO built by its constructor", () => {
  const it = interp();
  const raw = it.call("SIM_MODEL_RAW::SIM_MODEL_RAW", [], { arity: 1 });
  assert.deepEqual(raw.map((p) => [p.name, p.id, p.dir.name]), [["type", 1, "DIR_OUT"], ["lib", 1, "DIR_OUT"], ["lib", 1, "DIR_OUT"]]);
});

test("the ngspice tables: positional emplace_back rows, a short row's text where the constructor's next argument is", () => {
  const it = interp();
  const text = `
void NGSPICE_MODEL_INFO_MAP::addX()
{
    modelInfos[MODEL_TYPE::X] = { "Xname", "NX", "PX", { "A", "<B>" }, "An X, with a comma", {}, {} };
    modelInfos[MODEL_TYPE::X].modelParams.emplace_back( "is", 101, SIM_MODEL::PARAM::DIR_INOUT, SIM_VALUE::TYPE_FLOAT, "A", SIM_MODEL::PARAM::CATEGORY::DC, "1e-14", "", "Saturation, current" );
    modelInfos[MODEL_TYPE::X].modelParams.emplace_back( "vt0", 102, SIM_MODEL::PARAM::DIR_INOUT, SIM_VALUE::TYPE_FLOAT, "V", SIM_MODEL::PARAM::CATEGORY::DC, "Pinch-off voltage" );
    modelInfos[MODEL_TYPE::X].instanceParams.emplace_back( "off", 3, SIM_MODEL::PARAM::DIR_INOUT, SIM_VALUE::TYPE_BOOL /*x*/, "", SIM_MODEL::PARAM::CATEGORY::FLAGS, "", "", "Initially off", true );
}`;
  const x = readNgspiceData([text], it).get("X");
  assert.deepEqual([x.name, x.variant1, x.pinNames, x.description], ["Xname", "NX", ["A", "<B>"], "An X, with a comma"]);
  const plain = (p) => [p.name, p.id, p.category.name ?? p.category, p.defaultValue, p.description, p.isSpiceInstanceParam];
  assert.deepEqual(x.modelParams.map(plain), [
    ["is", 101, "DC", "1e-14", "Saturation, current", false],
    ["vt0", 102, "DC", "Pinch-off voltage", "", false],
  ]);
  assert.deepEqual(x.instanceParams.map(plain), [["off", 3, "FLAGS", "", "Initially off", true]]);
});

test("src/kicad/sim_models.json is what the extractor makes of KiCad's source (KICAD_SRC_DIR)", () => {
  const src = process.env.KICAD_SRC_DIR;
  if (!src || !existsSync(join(src, "eeschema/sim/sim_model.cpp"))) {
    console.log("skipped: set KICAD_SRC_DIR to KiCad's source to check the tables against it");
    return;
  }
  const json = JSON.parse(readFileSync(join(here, "..", "..", "src", "kicad", "sim_models.json"), "utf8"));
  assert.equal(json.commit, readFileSync(join(src, "COMMIT"), "utf8").trim(), "the tables were made from this KiCad");
  execFileSync(process.execPath, [join(here, "..", "extract-sim-models.js"), "--check"], { env: process.env, stdio: "inherit" });
});
