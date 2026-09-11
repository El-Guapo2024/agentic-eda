#!/usr/bin/env python3
"""
stats.py - scan sampled .kicad_pcb files and report layer count, board
outline size, footprint count, net count, track count, via count, and the
distance from each J*-prefixed footprint (connectors) to the nearest board
outline edge.

This is the statistic the main agentic-eda placement/routing engine needs
most: what fraction of connector (J*) footprints sit within 2 mm of a board
edge in real designs, so autoplacement can bias connectors toward edges
without being told to.

Usage:
    python3 stats.py <dir-of-kicad_pcb-files>
"""
import sys
import glob
import os
import sexpdata


def sx(node):
    """Convert a sexpdata node into a plain python list/atom tree, keeping
    Symbol names as plain strings for keys."""
    if isinstance(node, list):
        return [sx(n) for n in node]
    if isinstance(node, sexpdata.Symbol):
        return str(node)
    return node


def find_all(tree, tag):
    """Yield every list node in tree whose first element == tag (any depth)."""
    if isinstance(tree, list):
        if tree and tree[0] == tag:
            yield tree
        for child in tree:
            yield from find_all(child, tag)


def get_xy(pt_node):
    # pt_node like ['xy', x, y]
    return float(pt_node[1]), float(pt_node[2])


def parse_outline(tree):
    """Collect all Edge.Cuts line/arc/rect/poly segments as a list of
    (x1,y1,x2,y2) line segments, for nearest-edge distance computation."""
    segments = []
    pts_all = []

    def on_edge_cuts(node):
        for item in node:
            if isinstance(item, list) and item and item[0] == 'layer' and item[1] == 'Edge.Cuts':
                return True
        return False

    for gr_line in find_all(tree, 'gr_line'):
        if on_edge_cuts(gr_line):
            start = next((n for n in gr_line if isinstance(n, list) and n[0] == 'start'), None)
            end = next((n for n in gr_line if isinstance(n, list) and n[0] == 'end'), None)
            if start and end:
                x1, y1 = get_xy(start)
                x2, y2 = get_xy(end)
                segments.append((x1, y1, x2, y2))
                pts_all += [(x1, y1), (x2, y2)]

    for gr_rect in find_all(tree, 'gr_rect'):
        if on_edge_cuts(gr_rect):
            start = next((n for n in gr_rect if isinstance(n, list) and n[0] == 'start'), None)
            end = next((n for n in gr_rect if isinstance(n, list) and n[0] == 'end'), None)
            if start and end:
                x1, y1 = get_xy(start)
                x2, y2 = get_xy(end)
                # rectangle -> 4 edges
                segments += [(x1, y1, x2, y1), (x2, y1, x2, y2),
                             (x2, y2, x1, y2), (x1, y2, x1, y1)]
                pts_all += [(x1, y1), (x2, y2)]

    for gr_arc in find_all(tree, 'gr_arc'):
        if on_edge_cuts(gr_arc):
            # approximate arc by its start/mid/end points as a tiny segment fan
            pts = [n for n in gr_arc if isinstance(n, list) and n[0] in ('start', 'mid', 'end')]
            coords = [get_xy(p) for p in pts]
            for a, b in zip(coords, coords[1:]):
                segments.append((a[0], a[1], b[0], b[1]))
            pts_all += coords

    for gr_poly in find_all(tree, 'gr_poly'):
        if on_edge_cuts(gr_poly):
            pts_node = next((n for n in gr_poly if isinstance(n, list) and n[0] == 'pts'), None)
            if pts_node:
                coords = [get_xy(p) for p in pts_node if isinstance(p, list) and p[0] == 'xy']
                for a, b in zip(coords, coords[1:] + coords[:1]):
                    segments.append((a[0], a[1], b[0], b[1]))
                pts_all += coords

    return segments, pts_all


def point_to_segment_dist(px, py, x1, y1, x2, y2):
    dx, dy = x2 - x1, y2 - y1
    if dx == 0 and dy == 0:
        return ((px - x1) ** 2 + (py - y1) ** 2) ** 0.5
    t = ((px - x1) * dx + (py - y1) * dy) / (dx * dx + dy * dy)
    t = max(0.0, min(1.0, t))
    cx, cy = x1 + t * dx, y1 + t * dy
    return ((px - cx) ** 2 + (py - cy) ** 2) ** 0.5


def nearest_edge_dist(px, py, segments):
    if not segments:
        return None
    return min(point_to_segment_dist(px, py, *s) for s in segments)


def analyze_file(path):
    with open(path, "r", encoding="utf-8", errors="replace") as f:
        text = f.read()
    tree = sx(sexpdata.loads(text))

    layers_node = next((n for n in tree if isinstance(n, list) and n[0] == 'layers'), None)
    layer_count = len(layers_node) - 1 if layers_node else 0

    segments, pts_all = parse_outline(tree)
    if pts_all:
        xs = [p[0] for p in pts_all]
        ys = [p[1] for p in pts_all]
        width = max(xs) - min(xs)
        height = max(ys) - min(ys)
    else:
        width = height = None

    footprints = list(find_all(tree, 'footprint'))
    nets = list(find_all(tree, 'net'))
    # net 0 is always the implicit "no net" - real net count excludes it when present
    tracks = list(find_all(tree, 'segment'))
    vias = list(find_all(tree, 'via'))

    connector_distances = []
    for fp in footprints:
        # reference is in a property node: (property "Reference" "J3" ...)
        ref = None
        for prop in find_all(fp, 'property'):
            if len(prop) >= 3 and prop[1] == 'Reference':
                ref = prop[2]
                break
        if ref is None:
            # fallback: fp_text reference (older kicad syntax)
            for ft in find_all(fp, 'fp_text'):
                if len(ft) >= 3 and ft[1] == 'reference':
                    ref = ft[2]
                    break
        if not ref or not ref.startswith('J'):
            continue
        at_node = next((n for n in fp if isinstance(n, list) and n[0] == 'at'), None)
        if not at_node:
            continue
        fx, fy = float(at_node[1]), float(at_node[2])
        d = nearest_edge_dist(fx, fy, segments)
        if d is not None:
            connector_distances.append((ref, d))

    return {
        "file": os.path.basename(path),
        "layers": layer_count,
        "outline_w_mm": width,
        "outline_h_mm": height,
        "footprints": len(footprints),
        "nets": max(0, len(nets) - 1) if nets else 0,
        "tracks": len(tracks),
        "vias": len(vias),
        "connectors": connector_distances,
    }


def main():
    if len(sys.argv) < 2:
        print("usage: stats.py <dir>", file=sys.stderr)
        sys.exit(1)
    directory = sys.argv[1]
    files = sorted(glob.glob(os.path.join(directory, "*.kicad_pcb")))
    if not files:
        print(f"ERROR: no .kicad_pcb files found under {directory}", file=sys.stderr)
        sys.exit(1)

    all_connector_dists = []
    results = []
    for path in files:
        try:
            r = analyze_file(path)
        except Exception as e:
            print(f"FAILED to parse {path}: {e}", file=sys.stderr)
            continue
        results.append(r)
        all_connector_dists += [d for _, d in r["connectors"]]

    print(f"{'file':50s} {'layers':>6} {'W(mm)':>8} {'H(mm)':>8} {'fps':>5} {'nets':>6} {'tracks':>7} {'vias':>5} {'J*':>4}")
    for r in results:
        w = f"{r['outline_w_mm']:.1f}" if r['outline_w_mm'] is not None else "?"
        h = f"{r['outline_h_mm']:.1f}" if r['outline_h_mm'] is not None else "?"
        print(f"{r['file'][:50]:50s} {r['layers']:>6} {w:>8} {h:>8} {r['footprints']:>5} "
              f"{r['nets']:>6} {r['tracks']:>7} {r['vias']:>5} {len(r['connectors']):>4}")

    print()
    print("--- connector (J*) edge-distance detail ---")
    for r in results:
        for ref, d in r["connectors"]:
            flag = "<=2mm" if d <= 2.0 else ""
            print(f"{r['file'][:40]:40s} {ref:>6}  {d:7.2f} mm  {flag}")

    print()
    n = len(all_connector_dists)
    within_2mm = sum(1 for d in all_connector_dists if d <= 2.0)
    print(f"TOTAL J*-refdes footprints analyzed: {n}")
    if n:
        frac = within_2mm / n
        print(f"Within 2.0 mm of a board edge: {within_2mm}/{n} = {frac * 100:.1f}%")
        dists_sorted = sorted(all_connector_dists)
        median = dists_sorted[n // 2]
        print(f"Median distance to nearest edge: {median:.2f} mm")
        print(f"Max distance to nearest edge: {max(dists_sorted):.2f} mm")
    else:
        print("No J*-prefixed footprints found in sampled boards.")


if __name__ == "__main__":
    main()
