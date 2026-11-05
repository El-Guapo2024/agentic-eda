#!/usr/bin/env python3
"""Generates safety_power.yaml: the UV-laser PCB fab's safety + power board (2026-11-01 brief; design in README.md, parts in PARTS.md).
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
    if ref.startswith('J'): d['edge'] = True                  # every J on this board takes a cable from the machine
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
def FLY(k, a): D('1N4148W', k, a, lcsc='C81598')               # coil flyback, small coils (relays on this board)
def FLYC(k, a): D('SS34', k, a, pkg='SMA', lcsc='C8678')       # coil flyback, contactor coils off the board
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

# Omron G7SA-3A1B DC24: force-guided (EN 50205 class A), 3NO + 1NC. KiCad has no footprint, so it is drawn here from Omron's
# "Printed Circuit Board Design Diagram" (catalog J152-E1, p.5), turned to the top view. Two rows 10.16 apart; columns 0, 13.97,
# 19.05, 30.48, 35.56 from the coil pins; holes 1.4 dia; body 40 x 13, the coil column 1.83 in from its end. Origin = body centre.
G7SA_FP = 'Relay_Safety:Omron_G7SA-3A1B'
_cols = [-18170, -4200, 880, 12310, 17390]
_pads = [('1', 0, -5080), ('23', 1, -5080), ('24', 2, -5080), ('43', 3, -5080), ('44', 4, -5080),
         ('0', 0, 5080), ('11', 1, 5080), ('12', 2, 5080), ('33', 3, 5080), ('34', 4, 5080)]
FOOTPRINTS = [{'name': G7SA_FP, 'courtyard': [20250, 6750],
               'pads': [{'number': n, 'at': [_cols[c], y], 'size': [2400, 2400], 'shape': 'rect' if n == '0' else 'circle',
                         'kind': 'through_hole', 'drill': 1400} for n, c, y in _pads]}]
def G7SA(ref, coil_p, coil_n, contacts):
    """contacts: {'nc': (11, 12 nets), 'no1': (23, 24), 'no2': (33, 34), 'no3': (43, 44)}; an absent pole is left unconnected."""
    poles = {'nc': ('11', '12'), 'no1': ('23', '24'), 'no2': ('33', '34'), 'no3': ('43', '44')}
    used = {p for k in contacts for p in poles[k]}
    pins = [('0', 'passive', 'A1'), ('1', 'passive', 'A2')] + [(p, 'passive' if p in used else 'nc', p) for k in ('nc', 'no1', 'no2', 'no3') for p in poles[k]]
    part(ref, 'G7SA-3A1B DC24', pins, footprint=G7SA_FP, lcsc='C968857', mpn='G7SA-3A1B DC24')
    conn(ref, ('0', coil_p), ('1', coil_n), *[(p, n) for k, (a, b) in contacts.items() for p, n in zip(poles[k], (a, b))])
    FLY(coil_p, coil_n)

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
# control 24 V for the safety chain and the contactor coils (about 0.5 A), PTC-protected
part('F7', 'PTC 1.1A', ['1', '2'], footprint='Fuse:Fuse_1812_4532Metric', lcsc='C70119', mpn='mSMD110-33V', symbol='Device:Polyfuse'); conn('F7', ('1', 'V24'), ('2', 'V24_CTL'))

block('Switched 24V outputs')
# --- 24 V cut by RUN (E-stop, over-temperature, MCU stop) ---
def JQC(ref, out):
    part(ref, 'JQC-3FF-24V', [(11, 'passive', 'COM'), (12, 'nc', 'NC'), (14, 'passive', 'NO'), ('A1', 'passive', 'coil+'), ('A2', 'passive', 'coil-')],
         footprint='Relay_THT:Relay_SPDT_Hongfa_JQC-3FF_0XX-1Z', lcsc='C3638', mpn='JQC-3FF/24VDC-1HS(551)', symbol='Relay:JQC-3FF-024-1H')
    conn(ref, ('11', 'V24'), ('14', out), ('A1', 'RUN'), ('A2', 'GND')); FLY('RUN', 'GND')
JQC('K1', 'V24_MOT_SW'); FUSE('F2', '15A', 'V24_MOT_SW', 'V24_MOT'); J('J2', 'MOTION 24V', TB2, 'V24_MOT', 'GND', lcsc='C8465'); LED('V24_MOT')
JQC('K2', 'V24_VAC_SW'); FUSE('F3', '2A', 'V24_VAC_SW', 'V24_VAC'); J('J3', 'PUMP/VALVE 24V', TB2, 'V24_VAC', 'GND', lcsc='C8465'); LED('V24_VAC')  # pump 10 W + valve < 3 W: 0.5 A; the 0.5 mm track carries 2 A
block('Unswitched 24V outputs')
# --- 24 V that stays on after a stop ---
FUSE('F4', '5A', 'V24', 'V24_AUX'); J('J4', 'AUX 24V (fans, controller 5V supply)', TB2, 'V24_AUX', 'GND', lcsc='C8465'); LED('V24_AUX')
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

cnt['K'] = 2  # K1, K2 are the switched-output relays
block('Safety level 1: E-stop')
# --- level 1, two channels: E-stop NC1 (+ the hot-plate over-temperature cut-out) -> K3, E-stop NC2 -> K4; RESET latches both, each holds
# itself through its own NO; RUN = K3 NO in series with K4 NO. RESET only reaches the coils when every safety relay is off (their NCs) and
# every contactor is off (the mirror-contact loop on J11): a welded contact blocks the restart. The MCU can only drop RUN (PERMIT_LS). ---
J('J8', 'E-STOP (2 NC)', MC(4), 'V24_CTL', 'ES_A', 'V24_CTL', 'ES_B')
J('J9', 'HOT-PLATE CUT-OUT (NC)', MC(2), 'ES_A', 'OT_A')
J('J10', 'RESET (NO)', MC(2), 'OT_A', 'RST_IN')
J('J11', 'CONTACTOR MIRROR NCs (KM1, KMA, KMB in series)', MC(2), 'FB_IN', 'RST_OK')
G7SA('K3', 'L1A', 'PERMIT_LS', {'nc': ('RST_IN', 'RST_1'), 'no1': ('OT_A', 'L1A'), 'no2': ('V24_CTL', 'RUN_M')})
G7SA('K4', 'L1B', 'PERMIT_LS', {'nc': ('RST_1', 'RST_2'), 'no1': ('ES_B', 'L1B'), 'no2': ('RUN_M', 'RUN')})
FLY('L1A', 'RST_OK'); FLY('L1B', 'RST_OK')   # reset steering: RST_OK feeds both coils, neither coil feeds the other
J('J12', 'KM1 HOT-PLATE CONTACTOR COIL', MC(2), 'RUN', 'GND'); FLYC('RUN', 'GND')
LED('RUN', color='red')

block('Safety level 2: laser and UV lamps')
# --- level 2, two channels, fed from RUN: doors 1 and 2 + key (+ airflow on channel A, jumper bypass for tests) -> K5 / K6.
# K5 NO -> KMA coil, K6 NO -> KMB coil: KMA and KMB sit in series on the mains of the laser's 12 V supply and the lamp drivers.
# The lamps also need K5 NO + K6 NO in series to the KML coil, which the MCU switches (the drivers only dim to 10 %). ---
J('J13', 'DOOR 1 (2 NC)', MC(4), 'RUN', 'D1A', 'RUN', 'D1B')
J('J14', 'DOOR 2 (2 NC)', MC(4), 'D1A', 'D2A', 'D1B', 'D2B')
J('J15', 'KEY (2 NC)', MC(4), 'D2A', 'KYA', 'D2B', 'L2B')
J('J16', 'EXTRACTOR AIRFLOW (NO, closed = flowing)', MC(2), 'KYA', 'L2A')
J('JP1', 'AIRFLOW BYPASS (tests only)', 'Connector_PinHeader_2.54mm:PinHeader_1x02_P2.54mm_Vertical', 'KYA', 'L2A')
G7SA('K5', 'L2A', 'GND', {'nc': ('RST_2', 'RST_3'), 'no1': ('RUN', 'KMA_C'), 'no2': ('RUN', 'LAMP_M')})
G7SA('K6', 'L2B', 'GND', {'nc': ('RST_3', 'FB_IN'), 'no1': ('RUN', 'KMB_C'), 'no2': ('LAMP_M', 'LAMP_V')})
J('J17', 'KMA + KMB COILS (laser + lamp mains)', MC(4), 'KMA_C', 'GND', 'KMB_C', 'GND'); FLYC('KMA_C', 'GND'); FLYC('KMB_C', 'GND')
J('J18', 'KML LAMP CONTACTOR COIL', MC(2), 'LAMP_V', 'LAMP_LS'); FLYC('LAMP_V', 'LAMP_LS')
LED('KMA_C', color='amber')

block('MCU interface')
# --- the MCU's side, isolated (EL357N): 7 status lines in, 3 commands out (PERMIT can only drop RUN; LAMP and EXTRACTOR only switch
# coils whose power the safety chain already controls) ---
def OPTO(a_net, k_net, c_net, e_net, value='EL357N'):
    u = nx('U'); part(u, value, [(1, 'passive', 'A'), (2, 'passive', 'K'), (3, 'passive', 'E'), (4, 'passive', 'C')],
                      footprint='Package_SO:SOP-4_3.8x4.1mm_P2.54mm', lcsc='C42379244', mpn='EL357N(C)(TA)-G'); conn(u, ('1', a_net), ('2', k_net), ('3', e_net), ('4', c_net)); return u
def FET(g, s, d):
    q = nx('Q') if cnt['Q'] else (nx('Q'), nx('Q'))[1]
    part(q, 'AO3400A', [(1, 'signal', 'G'), (2, 'passive', 'S'), (3, 'passive', 'D')], package='SOT-23', lcsc='C20917', mpn='AO3400A'); conn(q, ('1', g), ('2', s), ('3', d))
def COMMAND(cmd, drain):
    """MCU pin -> opto -> 24 V side gate pull-up -> N-FET pulling `drain` low."""
    R('330', cmd, f'{cmd}_A'); OPTO(f'{cmd}_A', 'GND_MCU', f'{cmd}_P', f'{cmd}_G')
    R('22k', 'V24_CTL', f'{cmd}_P'); R('10k', f'{cmd}_G', 'GND'); FET(f'{cmd}_G', 'GND', drain)
nx('U'); nx('U'); nx('U')  # U1, U2 are the bucks, U3 the ideal diode
COMMAND('MCU_PERMIT', 'PERMIT_LS')
COMMAND('MCU_LAMP', 'LAMP_LS')
COMMAND('MCU_EXTR', 'EXTR_LS')
J('J19', 'EXTRACTOR RELAY COIL', MC(2), 'V24_AUX', 'EXTR_LS'); FLYC('V24_AUX', 'EXTR_LS')
rb = []
for src, name in [('ES_B', 'RB_ESTOP'), ('OT_A', 'RB_OVERTEMP'), ('D2A', 'RB_DOORS'), ('KYA', 'RB_KEY'), ('L2A', 'RB_AIRFLOW'), ('RUN', 'RB_RUN'), ('KMA_C', 'RB_LASER')]:
    R('10k', src, f'{name}_A'); OPTO(f'{name}_A', 'GND', name, 'GND_MCU'); R('10k', '3V3_MCU', name); rb.append(name)
J('J20', 'MCU', 'Connector_JST:JST_XH_B12B-XH-A_1x12_P2.50mm_Vertical', '3V3_MCU', 'GND_MCU', *rb, 'MCU_PERMIT', 'MCU_LAMP', 'MCU_EXTR')

block('Mechanical')
for h in range(1, 5): part(f'H{h}', 'M3', [('1', 'nc')], footprint='MountingHole:MountingHole_3.2mm_M3_Pad')

# LCSC numbers for the parts the helpers leave open, from the JLC parts database (2026-11-03): UNI-ROYAL 0603 1 % thick film,
# YAGEO/Samsung X7R, KENTO/NationStar 0603 LEDs, DEGSON 15EDGRC-3.81 right-angle headers (the Phoenix MC 1,5/..-G-3,81 pattern;
# check the DEGSON drawing against the KiCad footprint before ordering), JST XH, XFCN 2.54 header.
LCSC = {('10k', '0603'): 'C25804', ('49.9k', '0603'): 'C23184', ('100k', '0603'): 'C25803', ('4.12k', '0603'): 'C23041',
        ('17.8k', '0603'): 'C22821', ('1k', '0603'): 'C21190', ('330', '0603'): 'C23138', ('22k', '0603'): 'C31850',
        ('100nF 50V', '0603'): 'C14663', ('100nF', '0603'): 'C14663', ('22nF', '0603'): 'C21122',
        ('LED green', '0603'): 'C12624', ('LED amber', '0603'): 'C89811',
        MC(2): 'C507005', MC(4): 'C507007', 'Connector_JST:JST_XH_B12B-XH-A_1x12_P2.50mm_Vertical': 'C158027'}
for p in parts:
    if not p.get('lcsc'):
        l = LCSC.get((p['value'], p.get('package'))) or LCSC.get(p.get('footprint'))
        if l: p['lcsc'] = l
for p in parts:
    if p['reference'] == 'JP1' and not p.get('lcsc'): p['lcsc'] = 'C492401'

W, H = 220000, 160000
# Track widths on 2 oz copper (IPC-2221 outer): V24, the trunk to every fused branch, 2 mm (about 6 A at 20 C rise); the other
# power nets 1 mm (5 A fuses: about 20 C rise at 5 A). The nets that reach a SOT-23-6 or SO-8 pin (U1, U2, U3) route at 0.5 mm,
# the widest that enters those pads; V24's own small pads (U3.4, R1.1) get a hand-drawn neck. The 20 A path (VIN_RAW, VIN_F,
# V24 to K1, V24_MOT_SW, V24_MOT) is carried by F.Cu pours, added after routing with solid pad connections. Order 2 oz copper.
fine = ['VIN_F', 'V24_BUCK', 'SW1', 'SW2']
power = ['VIN_RAW', 'V24_MOT_SW', 'V24_MOT', 'V24_VAC_SW', 'V24_VAC', 'V24_AUX', 'V24_LED', 'V19', 'V5']
doc = {'parts': parts, 'nets': [{'name': n, 'pins': p} for n, p in nets.items()], 'footprints': FOOTPRINTS,
       'placement_rules': [{'kind': 'proximity', 'a': refs[0], 'b': r, 'max_mm': 50.0, 'reason': f'same block: {name}'} for name, refs in blocks.items() if name != 'Mechanical' for r in refs[1:]]
                          + [{'kind': 'proximity', 'a': a, 'b': b, 'max_mm': mx, 'reason': 'the 20 A path (supply in -> ideal diode -> motion relay -> fuse -> out) kept short'}
                             for a, b, mx in (('J1', 'F1', 15.0), ('F1', 'Q1', 15.0), ('Q1', 'K1', 30.0), ('K1', 'F2', 15.0), ('F2', 'J2', 15.0))]
                          + [{'kind': 'module', 'name': name, 'refs': refs} for name, refs in blocks.items() if name != 'Mechanical'],
       'board': {'layers': ['F.Cu', 'B.Cu'], 'outline': [{'x': 0, 'y': 0}, {'x': W, 'y': 0}, {'x': W, 'y': H}, {'x': 0, 'y': H}],
                 'net_classes': [{'name': 'trunk', 'nets': ['V24'], 'track_width': 2000}, {'name': 'power', 'nets': power, 'track_width': 1000},
                                 {'name': 'power_fine', 'nets': fine, 'track_width': 500}],
                 'pours': [{'net': 'GND', 'layer': 'B.Cu'}],
                 'tuning': {'optimizer_passes': 0}}}  # the FreeRouting port's optimiser takes > 50 min on this board; routing alone 40 s
print('# UV-laser PCB fab: safety + power board. Generated by gen_safety_power.py; edit that, not this.')
print(yaml.safe_dump(doc, sort_keys=False, width=200))
