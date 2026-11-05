"""render3d.py IN.kicad_pcb OUT.kicad_pcb MODELDIR: a copy of the board for the 3D render only. Parts with no KiCad 3D model get a
plain box the size of their courtyard (less 0.5 mm a side), at an approximate body height; the design files are not touched."""
import sys, re, os

src, dst, mdir = sys.argv[1:4]
os.makedirs(mdir, exist_ok=True)
# ref prefix / footprint keyword -> (height mm, rgb)
BOX = {'Relay_Safety:Omron_G7SA': (24.0, (0.12, 0.12, 0.14)),      # Omron G7SA, black
       'Relay_SPDT_Hongfa_JQC': (15.5, (0.15, 0.3, 0.75)),          # JQC-3FF, blue
       'Fuseholder_Blade_ATO': (12.0, (0.85, 0.85, 0.85)),          # ATO holder, light grey
       'Infineon_PG-TDSON-8': (1.0, (0.1, 0.1, 0.1)),
       'Fuse_1812_4532Metric': (1.0, (0.2, 0.2, 0.2))}
WRL = """#VRML V2.0 utf8
Shape {{ appearance Appearance {{ material Material {{ diffuseColor {r} {g} {b} specularColor 0.2 0.2 0.2 shininess 0.3 }} }}
  geometry IndexedFaceSet {{ coord Coordinate {{ point [ {pts} ] }}
    coordIndex [ 0 1 2 3 -1 7 6 5 4 -1 0 4 5 1 -1 1 5 6 2 -1 2 6 7 3 -1 3 7 4 0 -1 ] }} }}
"""
def close(t, i):
    """Index of the ')' that closes the '(' at t[i], skipping quoted strings."""
    depth, inq, esc = 0, False, False
    for k in range(i, len(t)):
        ch = t[k]
        if inq:
            if esc: esc = False
            elif ch == '\\': esc = True
            elif ch == '"': inq = False
        elif ch == '"': inq = True
        elif ch == '(': depth += 1
        elif ch == ')':
            depth -= 1
            if depth == 0: return k
    raise ValueError('unbalanced')

s = open(src).read()
out, i, made = [], 0, 0
for m in re.finditer(r'\n\t\(footprint "([^"]+)"', s):
    pass
parts = re.split(r'(?=\n\t\(footprint ")', s)
res = []
for blk in parts:
    m = re.match(r'\n\t\(footprint "([^"]+)"', blk)
    key = m and next((k for k in BOX if k in m.group(1)), None)
    if not key:
        res.append(blk); continue
    xs, ys = [], []
    for g in re.finditer(r'\(fp_(?:line|rect)\s+\(start ([-\d.]+) ([-\d.]+)\)\s+\(end ([-\d.]+) ([-\d.]+)\).*?\(layer "F\.CrtYd"\)', blk, re.S):
        a, b, c, d = map(float, g.groups()); xs += [a, c]; ys += [b, d]
    if not xs:
        for g in re.finditer(r'\(pad "[^"]*" \S+ \S+\s+\(at ([-\d.]+) ([-\d.]+)', blk):
            xs.append(float(g.group(1))); ys.append(float(g.group(2)))
        xs += [min(xs) - 1.5, max(xs) + 1.5]; ys += [min(ys) - 1.5, max(ys) + 1.5]
    x0, x1, y0, y1 = min(xs) + 0.5, max(xs) - 0.5, min(ys) + 0.5, max(ys) - 0.5
    h, (r, g_, b) = BOX[key]
    # KiCad WRL units are 0.1 inch; footprint y is down, model y is up
    u = lambda v: v / 2.54
    pts = ', '.join(f'{u(x):.4f} {u(-y):.4f} {u(z):.4f}' for z in (0, h) for x, y in ((x0, y0), (x1, y0), (x1, y1), (x0, y1)))
    name = re.sub(r'[^A-Za-z0-9_.-]', '_', m.group(1)) + '.wrl'
    open(os.path.join(mdir, name), 'w').write(WRL.format(r=r, g=g_, b=b, pts=pts))
    model = f'\n\t\t(model "{os.path.join(mdir, name)}"\n\t\t\t(offset (xyz 0 0 0))\n\t\t\t(scale (xyz 1 1 1))\n\t\t\t(rotate (xyz 0 0 0))\n\t\t)'
    while (j := blk.find('\n\t\t(model "')) >= 0:
        blk = blk[:j] + blk[close(blk, j + 3) + 1:]
    # insert before the footprint's closing paren: the block ends with "\n\t)" for the footprint
    k = close(blk, blk.index('('))
    blk = blk[:k] + model + '\n\t' + blk[k:]
    made += 1
    res.append(blk)
    continue
    for k, ch in enumerate(blk):
        if inq:
            if esc: esc = False
            elif ch == '\\': esc = True
            elif ch == '"': inq = False
            continue
        if ch == '"': inq = True
        elif ch == '(': depth += 1
        elif ch == ')':
            depth -= 1
            if depth == 0: break
    blk = blk[:k] + model + '\n\t' + blk[k:]
    made += 1
    res.append(blk)
open(dst, 'w').write(''.join(res))
print(f'{made} placeholder bodies')
