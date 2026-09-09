export const meta = {
  name: 'eda-loop-round',
  description: 'One improvement round: judge every board per stage, verify defects adversarially, improve placement/routing, check for regressions',
  phases: [
    { title: 'Measure', detail: 'one judge agent per board: run pipeline, look at the renders, list defects' },
    { title: 'Verify', detail: 'one skeptic per stage tries to refute each defect' },
    { title: 'Improve', detail: 'placement and routing agents fix confirmed defects (schematic crates are busy)' },
    { title: 'Regress', detail: 'full corpus + workspace tests, compare with the measurement' },
  ],
}
const REPO = '/Users/juanantonioluera/ws/agentic-eda'
const SCRATCH = '/private/tmp/claude-501/-Users-juanantonioluera/0fb616fd-320e-42a1-aa73-8d2cd469c509/scratchpad/wf_round'
const BOARDS = args && args.boards ? args.boards : [
  'examples/ldo.yaml','examples/ldo_proximity_heavy.yaml','examples/opamp_filter.yaml','examples/through_hole_headers.yaml',
  'examples/mcu_board_30plus.yaml','examples/passive_divider_ladder.yaml','examples/star_net.yaml','examples/nc_pins.yaml',
  'examples/two_pin_nets.yaml','examples/all_power_ground_net.yaml','examples/dense_small_outline.yaml',
  'examples/ladder/l1_usb_mcu.yaml','examples/ladder/l2_sensor_hub.yaml','examples/ladder/l3_motor_hub.yaml','examples/ladder/l4_control_hub.yaml',
]
const MEASURE = {
  type: 'object', required: ['board','exists','stage_reached','gate_fails','defects','scores'],
  properties: {
    board: {type:'string'}, exists: {type:'boolean'},
    stage_reached: {type:'string', enum:['none','schematic','placement','routing','done']},
    gate_fails: {type:'array', items:{type:'string'}},
    scores: {type:'object', properties:{schematic:{type:'number'},placement:{type:'number'},routing:{type:'number'}}},
    defects: {type:'array', items:{type:'object', required:['stage','check','severity','location','hint'], properties:{
      stage:{type:'string', enum:['schematic','placement','routing']}, check:{type:'string'}, severity:{type:'string', enum:['fail','warn']},
      location:{type:'string'}, hint:{type:'string'}}}},
  },
}
const VERDICTS = { type:'object', required:['verdicts'], properties:{ verdicts:{type:'array', items:{type:'object', required:['board','check','location','real','reason'], properties:{
  board:{type:'string'}, check:{type:'string'}, location:{type:'string'}, real:{type:'boolean'}, reason:{type:'string'}}}}}}
const IMPROVE = { type:'object', required:['stage','fixed','not_fixed','gates_added','files_changed','notes'], properties:{
  stage:{type:'string'}, fixed:{type:'array', items:{type:'string'}}, not_fixed:{type:'array', items:{type:'string'}},
  gates_added:{type:'array', items:{type:'string'}}, files_changed:{type:'array', items:{type:'string'}}, notes:{type:'string'}}}
const REGRESS = { type:'object', required:['tests_green','regressions','improvements','table'], properties:{
  tests_green:{type:'boolean'}, regressions:{type:'array', items:{type:'string'}}, improvements:{type:'array', items:{type:'string'}}, table:{type:'string'}}}

phase('Measure')
const measured = (await pipeline(BOARDS, (b, _, i) => agent(
`You are the visual judge for one board of the Rust EDA toolkit at ${REPO}. Board intent: ${b}. If the file does not exist, return exists=false and empty lists.
Do: cd ${REPO}; mkdir -p ${SCRATCH}/${i}; ./target/debug/eda pipeline ${b} -o ${SCRATCH}/${i} --seed 0 > ${SCRATCH}/${i}/log.txt 2>&1 (do NOT cargo build; the binary exists). Then ./target/debug/eda judge ${b} --design ${SCRATCH}/${i}/design.json -o ${SCRATCH}/${i} (it will fail with judge_unavailable, that is expected; it still writes judge_schematic.png, judge_placement.png, judge_routing.png for the stages present).
Read log.txt for gate results (record every [Fail] check name in gate_fails, and stage_reached). Then LOOK at each judge_*.png with the Read tool and judge it as a senior EE signing off: schematic rubric = labels next to their part, no text over wires/text, short direct wires, power/ground as flags, left-to-right flow, related parts grouped, few crossings, junction dots; placement rubric = decoupling next to IC pins, connectors on the edge facing out, connected parts close, even board use, consistent orientation, room to route; routing rubric = short direct traces, few vias, nothing between/under pads, clean corners, not hugging the edge. For each concrete defect name the refdes/net. Score each stage present 0-10 (10 = sign-off). Do not edit any file.`,
  { label: `judge:${b.split('/').pop()}`, phase: 'Measure', schema: MEASURE, effort: 'low' }))).filter(Boolean).filter(m => m.exists)
log(`measured ${measured.length} boards; gate fails on ${measured.filter(m => m.gate_fails.length).length}`)

phase('Verify')
const byStage = {}
for (const m of measured) for (const d of m.defects) (byStage[d.stage] ||= []).push({ board: m.board, ...d })
const verified = {}
await parallel(Object.keys(byStage).map(stage => async () => {
  const list = byStage[stage]
  const v = await agent(
`You are a skeptical senior EE. Repo ${REPO}. Renders live under ${SCRATCH}/<index>/judge_${stage}.png (index = position of the board in this list: ${JSON.stringify(BOARDS)}). For EACH defect below, open the render and try to REFUTE it: is it really visible and really a defect a reviewer would send back? Mark real=false if you cannot see it or it is a nit. Do not edit files.
Defects: ${JSON.stringify(list)}`,
    { label: `refute:${stage}`, phase: 'Verify', schema: VERDICTS })
  verified[stage] = (v ? v.verdicts : []).filter(x => x.real)
  log(`${stage}: ${list.length} claimed, ${verified[stage].length} confirmed`)
}))

phase('Improve')
const improveTargets = ['placement', 'routing'].filter(s => (verified[s] || []).length || measured.some(m => m.gate_fails.some(g => g.startsWith(s))))
const scope = {
  placement: 'crates/place and placement_* functions in crates/gates/src/pcb.rs',
  routing: 'crates/router and routing_* functions in crates/gates/src/pcb.rs',
}
const improved = (await parallel(improveTargets.map(stage => () => agent(
`You are the ${stage} improvement agent for ${REPO}. Work ONLY in ${scope[stage]}. Do not touch crates/layout, crates/engine, crates/render, crates/gates/src/lib.rs, crates/judge, examples/ (other agents are editing them) and do NOT git commit.
Confirmed defects from the visual judge (fix these, each one FIRST as a gate that fails with a numeric threshold, then the engine change until it passes): ${JSON.stringify(verified[stage] || [])}.
Gate failures already present: ${JSON.stringify(measured.flatMap(m => m.gate_fails.filter(g => g.startsWith(stage)).map(g => m.board + ': ' + g)))}.
Do at least 4 numbered experiments; keep a change only if no board in examples/*.yaml examples/ladder/*.yaml regresses on gates, routability, or kicad-cli DRC (/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli pcb drc after eda export). cargo test -q -p eda-${stage === 'placement' ? 'place' : 'router'} -p eda-gates must be green; never weaken thresholds. Render with ./target/debug/eda judge ... to get judge_${stage}.png and LOOK at it after each change.`,
  { label: `improve:${stage}`, phase: 'Improve', schema: IMPROVE })))).filter(Boolean)
if (!improveTargets.length) log('nothing confirmed for placement/routing; schematic defects are handed to the running schematic agent via the report')

phase('Regress')
const regress = await agent(
`Repo ${REPO}. Rebuild (cargo build -q -p eda-cli) and run bash bench/loop/run.sh ${SCRATCH}/after 0 (all examples and ladder). Then cargo test -q --workspace. Compare the gate results with this pre-round measurement: ${JSON.stringify(measured.map(m => ({board: m.board, stage_reached: m.stage_reached, gate_fails: m.gate_fails})))}. A board that reaches a later stage or loses a fail is an improvement; one that reaches an earlier stage or gains a fail is a regression. Do not edit files. Return a markdown table (board, before, after).`,
  { label: 'regression-check', phase: 'Regress', schema: REGRESS })

return {
  measured: measured.map(m => ({ board: m.board, stage_reached: m.stage_reached, gate_fails: m.gate_fails, scores: m.scores })),
  confirmed_defects: verified,
  improved,
  regress,
}