#!/usr/bin/env python3
"""Generates safety_power.yaml: the desktop PCB fab's safety + power board (option A, 2026-10-28).
Lengths in um (intent schema 1). Re-run after edits: python3 gen_safety_power.py > safety_power.yaml"""
import yaml, collections
parts, nets = [], collections.defaultdict(list)
BLOCK = [None]; blocks = collections.OrderedDict()
def block(name): BLOCK[0] = name
P = lambda n, k='passive', nm=None: {k2: v for k2, v in (('number', str(n)), ('name', nm), ('kind', k)) if v is not None}
def part(ref, value, pins, package=None, footprint=None, lcsc=None, mpn=None, symbol=None):
    d = {'reference': ref, 'value': value}
    for k, v in (('mpn', mpn), ('lcsc', lcsc), ('package', package), ('footprint', footprint), ('symbol', symbol)):
        if v: d[k] = v
    d['pins'] = [P(*p) if isinstance(p, tuple) else P(p) for p in pins]; parts.append(d)
    if BLOCK[0]: blocks.setdefault(BLOCK[0], []).append(ref)
def conn(ref, *pairs):
    for pin, net in pairs:
        if net: nets[net].append(f'{ref}.{pin}')
cnt = collections.Counter()
def nx(prefix): cnt[prefix] += 1; return f'{prefix}{cnt[prefix]}'
def R(val, a, b, lcsc=None, pkg='0603'):
    r = nx('R'); part(r, val, ['1', '2'], package=pkg, lcsc=lcsc); conn(r, ('1', a), ('2', b)); return r
def C(val, a, b, pkg='0603', lcsc=None):
    c = nx('C'); part(c, val, ['1', '2'], package=pkg, lcsc=lcsc); conn(c, ('1', a), ('2', b)); return c
def CP(val, plus, minus):
    c = nx('C'); part(c, val, [(1, 'passive', '+'), (2, 'passive', '-')], footprint='Capacitor_SMD:CP_Elec_6.3x7.7', lcsc='C72478', mpn='RVT1V101M0607', symbol='Device:C_Polarized'); conn(c, ('1', plus), ('2', minus))
def D(val, k, a, pkg='SOD-123', fp=None, lcsc=None, mpn=None):
    d = nx('D'); part(d, val, [(1, 'passive', 'K'), (2, 'passive', 'A')], package=None if fp else pkg, footprint=fp, lcsc=lcsc, mpn=mpn, symbol='Device:D'); conn(d, ('1', k), ('2', a))
def LED(net, r='10k', color='green'):
    a = f'{net}_LED'; R(r, net, a); d = nx('D')
    part(d, f'LED {color}', [(1, 'passive', 'K'), (2, 'passive', 'A')], package='0603', lcsc='C2286' if color == 'red' else None, symbol='Device:LED'); conn(d, ('1', 'GND'), ('2', a))
TB2 = 'TerminalBlock_Phoenix:TerminalBlock_Phoenix_MKDS-1,5-2-5.08_1x02_P5.08mm_Horizontal'
MC = lambda n: f'Connector_Phoenix_MC:PhoenixContact_MC_1,5_{n}-G-3.81_1x0{n}_P3.81mm_Horizontal'
ATO = 'Fuse:Fuseholder_Blade_ATO_Littelfuse_Pudenz_2_Pin'
def J(ref, value, fp, *nets_, lcsc=None):
    part(ref, value, [str(i + 1) if n else (str(i + 1), 'nc') for i, n in enumerate(nets_)], footprint=fp, lcsc=lcsc); conn(ref, *[(str(i + 1), n) for i, n in enumerate(nets_)])
def FUSE(ref, rating, a, b):
    part(ref, f'ATO {rating}', ['1', '2'], footprint=ATO, lcsc='C668119', mpn='XF-507P', symbol='Device:Fuse'); conn(ref, ('1', a), ('2', b))

block('Power input')
# --- 24 V input: main fuse, bidirectional TVS, ideal-diode reverse-polarity protection (MX74700T, -100 V rated, + 1.6 mOhm FET) ---
J('J1', '24V IN', TB2, 'VIN_RAW', 'GND', lcsc='C8465')
FUSE('F1', '20A', 'VIN_RAW', 'VIN_F')
D('SMCJ33CA', 'VIN_F', 'GND', fp='Diode_SMD:D_SMC', lcsc='C310048', mpn='SMCJ33CA/TR13')
part('U3', 'MX74700T', [(1, 'passive', 'VCAP'), (2, 'ground', 'GND'), (3, 'signal', 'EN'), (4, 'passive', 'CATHODE'), (5, 'signal', 'GATE'), (6, 'passive', 'ANODE')],
     package='SOT-23-6', lcsc='C22468156', mpn='MX74700T')
conn('U3', ('1', 'IDEAL_VCAP'), ('2', 'GND'), ('3', 'VIN_F'), ('4', 'V24'), ('5', 'IDEAL_GATE'), ('6', 'VIN_F'))
C('100nF 50V', 'IDEAL_VCAP', 'VIN_F')
part('Q1', 'BSC016N06NS', [(1, 'passive', 'S'), (2, 'passive', 'S'), (3, 'passive', 'S'), (4, 'signal', 'G'), (5, 'passive', 'D')],
     footprint='Package_SON:Infineon_PG-TDSON-8_6.15x5.15mm', lcsc='C454269', mpn='BSC016N06NS')
conn('Q1', ('1', 'VIN_F'), ('2', 'VIN_F'), ('3', 'VIN_F'), ('4', 'IDEAL_GATE'), ('5', 'V24'))
CP('100uF 35V', 'V24', 'GND'); CP('100uF 35V', 'V24', 'GND')
LED('V24')

block('Switched 24V outputs')
# --- switched 24 V (cut by E-stop through RUN) ---
def JQC(ref, out):
    part(ref, 'JQC-3FF-24V', [(11, 'passive', 'COM'), (12, 'nc', 'NC'), (14, 'passive', 'NO'), ('A1', 'passive', 'coil+'), ('A2', 'passive', 'coil-')],
         footprint='Relay_THT:Relay_SPDT_Hongfa_JQC-3FF_0XX-1Z', lcsc='C3638', mpn='JQC-3FF/24VDC-1HS(551)', symbol='Relay:JQC-3FF-024-1H')
    conn(ref, ('11', 'V24'), ('14', out), ('A1', 'RUN'), ('A2', 'GND')); D('1N4148W', 'RUN', 'GND', lcsc='C81598')
JQC('K1', 'V24_MOT_SW'); FUSE('F2', '15A', 'V24_MOT_SW', 'V24_MOT'); J('J2', 'MOTION 24V', TB2, 'V24_MOT', 'GND', lcsc='C8465'); LED('V24_MOT')
JQC('K2', 'V24_TH_SW'); FUSE('F3', '10A', 'V24_TH_SW', 'V24_TH'); J('J3', 'TOOLHEAD 24V', TB2, 'V24_TH', 'GND', lcsc='C8465'); LED('V24_TH')
block('Unswitched 24V outputs')
# --- unswitched 24 V ---
FUSE('F4', '5A', 'V24', 'V24_AUX'); J('J4', 'FANS/PUMPS 24V', TB2, 'V24_AUX', 'GND', lcsc='C8465'); LED('V24_AUX')
FUSE('F5', '3A', 'V24', 'V24_LED'); J('J5', 'LED STRIP 24V', TB2, 'V24_LED', 'GND', lcsc='C8465'); LED('V24_LED')
FUSE('F6', '5A', 'V24', 'V24_BUCK')

# --- bucks: TI LMR16030 (4.3-60 V in, VFB 0.75 V), 500 kHz (RT 49.9k), 10 uH ---
def BUCK(ref, out, rtop, rbot):
    i = ref[1:]
    part(ref, 'LMR16030SDDA', [(1, 'signal', 'BOOT'), (2, 'power', 'VIN'), (3, 'nc', 'EN'), (4, 'signal', 'RT/SYNC'), (5, 'signal', 'FB'), (6, 'signal', 'SS'), (7, 'ground', 'GND'), (8, 'power', 'SW'), (9, 'ground', 'PAD')],
         footprint='Package_SO:TI_SO-PowerPAD-8', lcsc='C136648', mpn='LMR16030SDDAR')
    conn(ref, ('1', f'BOOT{i}'), ('2', 'V24_BUCK'), ('4', f'RT{i}'), ('5', f'FB{i}'), ('6', f'SS{i}'), ('7', 'GND'), ('8', f'SW{i}'), ('9', 'GND'))
    C('10uF 50V', 'V24_BUCK', 'GND', pkg='1206', lcsc='C13585'); C('10uF 50V', 'V24_BUCK', 'GND', pkg='1206', lcsc='C13585'); C('100nF 50V', 'V24_BUCK', 'GND')
    C('100nF', f'BOOT{i}', f'SW{i}'); R('49.9k', f'RT{i}', 'GND'); C('22nF', f'SS{i}', 'GND')
    D('SS56C', f'SW{i}', 'GND', fp='Diode_SMD:D_SMC', lcsc='C24138', mpn='SS56C')
    l = nx('L'); part(l, '10uH 4.5A', ['1', '2'], footprint='Inductor_SMD:L_Changjiang_FXL0630', lcsc='C167223', mpn='FXL0630-100-M', symbol='Device:L'); conn(l, ('1', f'SW{i}'), ('2', out))
    for _ in range(3): C('10uF 50V', out, 'GND', pkg='1206', lcsc='C13585')
    R(rtop, out, f'FB{i}'); R(rbot, f'FB{i}', 'GND')
block('Buck 19V Jetson')
BUCK('U1', 'V19', '100k', '4.12k')   # 0.75 * (1 + 100/4.12) = 18.95 V
J('J6', 'JETSON 19V', TB2, 'V19', 'GND', lcsc='C8465'); LED('V19')
block('Buck 5V')
BUCK('U2', 'V5', '100k', '17.8k')    # 0.75 * (1 + 100/17.8) = 4.96 V
J('J7', 'CAMERAS 5V', TB2, 'V5', 'GND', lcsc='C8465'); LED('V5', r='1k')

block('Laser chain')
# --- control 24 V, PTC-protected ---
part('F7', 'PTC 1.1A', ['1', '2'], footprint='Fuse:Fuse_1812_4532Metric', lcsc='C70119', mpn='mSMD110-33V', symbol='Device:Polyfuse'); conn('F7', ('1', 'V24'), ('2', 'V24_CTL'))

# --- laser chain (contact A of every switch, dry, in series, to the laser source's interlock) ---
sw = [('J8', 'E-STOP', 'ESTOP_B'), ('J9', 'DOOR 1', 'DOOR1_B'), ('J10', 'DOOR 2', 'DOOR2_B'), ('J11', 'KEY', 'KEY_B'), ('J12', 'TOOL LOCK', 'TOOL_B')]
for i, (ref, name, b) in enumerate(sw):
    J(ref, name, MC(4), f'LCH{i}', f'LCH{i + 1}', 'V24_CTL', b)
def G5V(ref, pins):
    part(ref, 'G5V-2-DC24', [(1, 'passive', 'coil'), (16, 'passive', 'coil'), (4, 'passive', 'COM1'), (6, 'passive', 'NC1'), (8, 'passive', 'NO1'), (13, 'passive', 'COM2'), (11, 'passive', 'NC2'), (9, 'passive', 'NO2')],
         footprint='Relay_THT:Relay_DPDT_Omron_G5V-2', lcsc='C39593', mpn='G5V-2-DC24', symbol='Relay:G5V-2')
    for p in parts[-1]['pins']:
        if p['number'] not in pins: p['kind'] = 'nc'
    conn(ref, *pins.items())
G5V('K3', {'1': 'V24_CTL', '16': 'FANOK_COIL', '4': 'LCH5', '8': 'LCH6'})   # fan-OK contact closes the chain
J('JP1', 'FAN BYPASS (test)', 'Connector_PinHeader_2.54mm:PinHeader_1x02_P2.54mm_Vertical', 'LCH5', 'LCH6')
J('J13', 'LASER INTERLOCK', MC(2), 'LCH0', 'LCH6')

block('Machine power latch')
# --- machine power latch: E-stop contact B + reset -> K4 self-hold -> RUN; MCU can only drop it ---
G5V('K4', {'1': 'LATCH_COIL', '16': 'PERMIT_LS', '4': 'ESTOP_B', '8': 'LATCH_COIL', '13': 'V24_CTL', '9': 'RUN'})
D('1N4148W', 'LATCH_COIL', 'PERMIT_LS', lcsc='C81598')
J('J14', 'RESET BUTTON', MC(2), 'ESTOP_B', 'LATCH_COIL')
J('J15', 'MAINS CONTACTOR COIL', MC(2), 'RUN', 'GND')
D('SS34', 'RUN', 'GND', pkg='SMA', lcsc='C8678')
LED('RUN', color='red')

block('MCU readback')
# --- isolated readback / permit to the MCU (EL357N) ---
def OPTO(a_net, k_net, c_net, e_net, value='EL357N'):
    u = nx('U'); part(u, value, [(1, 'passive', 'A'), (2, 'passive', 'K'), (3, 'passive', 'E'), (4, 'passive', 'C')],
                      footprint='Package_SO:SOP-4_3.8x4.1mm_P2.54mm', lcsc='C42379244', mpn='EL357N(C)(TA)-G'); conn(u, ('1', a_net), ('2', k_net), ('3', e_net), ('4', c_net)); return u
def FET(kind, g, s, d):
    q = nx('Q') if cnt['Q'] else (nx('Q'), nx('Q'))[1]; n = kind == 'N'
    part(q, 'AO3400A' if n else 'AO3401A', [(1, 'signal', 'G'), (2, 'passive', 'S'), (3, 'passive', 'D')], package='SOT-23', lcsc='C20917' if n else 'C15127', mpn='AO3400A' if n else 'AO3401A')
    conn(q, ('1', g), ('2', s), ('3', d))
nx('U'); nx('U'); nx('U')  # U1, U2 are the bucks, U3 the ideal diode
R('330', 'MCU_PERMIT', 'PERMIT_A'); OPTO('PERMIT_A', 'GND_MCU', 'V24_CTL_P', 'PERMIT_G')
R('22k', 'V24_CTL', 'V24_CTL_P'); R('10k', 'PERMIT_G', 'GND'); FET('N', 'PERMIT_G', 'GND', 'PERMIT_LS')
rb = []
for src, name, r in [('ESTOP_B', 'RB_ESTOP', '10k'), ('DOOR1_B', 'RB_DOOR1', '10k'), ('DOOR2_B', 'RB_DOOR2', '10k'), ('KEY_B', 'RB_KEY', '10k'),
                     ('TOOL_B', 'RB_TOOL', '10k'), ('RUN', 'RB_RUN', '10k'), ('FANOK', 'RB_FAN', '680'), ('SSR_P', 'RB_HEAT', '10k')]:
    R(r, src, f'{name}_A'); OPTO(f'{name}_A', 'GND', name, 'GND_MCU'); R('10k', '3V3_MCU', name); rb.append(name)
J('J16', 'MCU', 'Connector_JST:JST_XH_B12B-XH-A_1x12_P2.50mm_Vertical', '3V3_MCU', 'GND_MCU', *rb, 'MCU_PERMIT', 'GND_MCU')

block('Fan check')
# --- fume-fan tach -> retriggerable one-shot (220 ms) -> K3 ---
J('J17', 'FAN TACH', MC(2), 'TACH', 'GND')
R('10k', 'V5', 'TACH')
part('U' + str(cnt['U'] + 1), 'SN74LVC1G123', [(1, 'signal', '~A'), (2, 'signal', 'B'), (3, 'signal', '~CLR'), (4, 'ground', 'GND'), (5, 'signal', 'Q'), (6, 'passive', 'Cext'), (7, 'passive', 'RCext'), (8, 'power', 'VCC')],
     footprint='Package_SO:SSOP-8_2.95x2.8mm_P0.65mm', lcsc='C123302', mpn='SN74LVC1G123DCTR', symbol='74xGxx:74LVC1G123'); u = parts[-1]['reference']; nx('U')
conn(u, ('1', 'GND'), ('2', 'TACH'), ('3', 'V5'), ('4', 'GND'), ('5', 'FANOK'), ('6', 'CEXT'), ('7', 'RCEXT'), ('8', 'V5'))
C('2.2uF', 'CEXT', 'RCEXT'); R('100k', 'RCEXT', 'V5'); C('100nF', 'V5', 'GND'); R('100k', 'FANOK', 'GND')
FET('N', 'FANOK', 'GND', 'FANOK_COIL'); D('1N4148W', 'V24_CTL', 'FANOK_COIL', lcsc='C81598')

block('Hot-plate over-temp')
# --- hot-plate over-temperature (ratiometric, fail-safe on open and short), gates the SSR feed ---
J('J18', 'PLATE NTC 100k', MC(2), 'NTC_N', 'GND')
R('10k 1%', 'V5', 'NTC_N'); C('100nF', 'NTC_N', 'GND')
R('39k 1%', 'V5', 'HOT_REF'); R('1k 1%', 'HOT_REF', 'GND')      # 0.125 V: NTC 100k/3950 at ~270 C
R('1k 1%', 'V5', 'OPEN_REF'); R('100k 1%', 'OPEN_REF', 'GND')    # 4.95 V: sensor open (colder than about -10 C)
part('U' + str(cnt['U'] + 1), 'LM393', [(1, 'signal', 'OUT1'), (2, 'signal', 'IN1-'), (3, 'signal', 'IN1+'), (4, 'ground', 'GND'), (5, 'signal', 'IN2+'), (6, 'signal', 'IN2-'), (7, 'signal', 'OUT2'), (8, 'power', 'VCC')],
     package='SOIC-8', lcsc='C7955', mpn='LM393DR2G', symbol='Comparator:LM393'); u = parts[-1]['reference']; nx('U')
conn(u, ('1', 'OT_OK'), ('2', 'HOT_REF'), ('3', 'NTC_N'), ('4', 'GND'), ('5', 'OPEN_REF'), ('6', 'NTC_N'), ('7', 'OT_OK'), ('8', 'V5'))
R('10k', 'V5', 'OT_OK'); C('100nF', 'V5', 'GND')
FET('N', 'OT_OK', 'GND', 'HEAT_PD'); R('6.8k', 'HEAT_G', 'HEAT_PD'); R('10k', 'RUN', 'HEAT_G'); FET('P', 'HEAT_G', 'RUN', 'SSR_P')
J('J19', 'SSR OUT', MC(2), 'SSR_P', 'SSR_N')
J('J20', 'HEATER FROM MCU', MC(2), None, 'SSR_N')

block('CAN pass-through')
# --- CAN pass-through to the tool head ---
J('J21', 'CAN IN', MC(2), 'CANH', 'CANL'); J('J22', 'CAN TO TOOLHEAD', MC(2), 'CANH', 'CANL')
block('Mechanical')
for h in range(1, 5): part(f'H{h}', 'M3', [('1', 'nc')], footprint='MountingHole:MountingHole_3.2mm_M3_Pad')

W, H = 100000, 80000
power = ['VIN_RAW', 'VIN_F', 'V24', 'V24_MOT_SW', 'V24_MOT', 'V24_TH_SW', 'V24_TH', 'V24_AUX', 'V24_LED', 'V24_BUCK', 'V19', 'V5', 'SW1', 'SW2']
doc = {'parts': parts, 'nets': [{'name': n, 'pins': p} for n, p in nets.items()],
       'placement_rules': [{'kind': 'proximity', 'a': refs[0], 'b': r, 'max_mm': 30.0, 'reason': f'same block: {name}'} for name, refs in blocks.items() if name != 'Mechanical' for r in refs[1:]]
                          + [{'kind': 'module', 'name': name, 'refs': refs} for name, refs in blocks.items()],
       'board': {'layers': ['F.Cu', 'B.Cu'], 'outline': [{'x': 0, 'y': 0}, {'x': W, 'y': 0}, {'x': W, 'y': H}, {'x': 0, 'y': H}],
                 'net_classes': [{'name': 'power', 'nets': power, 'track_width': 1500}],
                 'pours': [{'net': 'GND', 'layer': 'B.Cu'}]}}
print('# Desktop PCB fab: safety + power board (option A). Generated by gen_safety_power.py; edit that, not this.')
print(yaml.safe_dump(doc, sort_keys=False, width=200))
