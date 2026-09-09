#!/usr/bin/env python3
"""Render a loop round directory (from bench/loop/run.sh) as one HTML page:
per board the schematic, placement and routing renders, gate status per
stage, judge verdicts when present, and an optional 'before' schematic
(pass a previous round dir as argv[2])."""
import os, re, glob, html, base64, json, sys
OUT = sys.argv[1]; PREV = sys.argv[2] if len(sys.argv) > 2 else None
def svg(p):
    s = open(p).read(); s = re.sub(r'<\?xml[^>]*\?>', '', s)
    return s.replace("<svg ", "<svg style=\"width:100%;height:auto;max-height:560px\" ", 1)
def png(p):
    return '<img src="data:image/png;base64,%s" alt="">' % base64.b64encode(open(p, "rb").read()).decode()
def gate(log, st):
    m = re.search(st + r" gates: (\d+) checks, (\d+) fail, (\d+) warn", log)
    if not m: return '<td class="na">not reached</td>'
    c, f, w = m.groups(); cls = "fail" if int(f) else ("warn" if int(w) else "ok")
    return f'<td class="{cls}">{f} fail · {w} warn</td>'
rows, cards = [], []
for d in sorted(glob.glob(OUT + "/*/")):
    n = os.path.basename(d.rstrip("/"))
    if not os.path.exists(d + "log.txt"): continue
    log = open(d + "log.txt").read()
    intent = open(d + "intent.yaml").read() if os.path.exists(d + "intent.yaml") else ""
    parts = len(re.findall(r"^\s+- reference", intent, re.M)); nets = len(re.findall(r"^\s+- \{?\s*name:", intent, re.M))
    fails = [html.escape(l.strip()[7:]) for l in log.splitlines() if "[Fail]" in l]
    warns = [html.escape(l.strip()[7:]) for l in log.splitlines() if "[Warn]" in l]
    cross = re.search(r"(\d+) wire/wire", log); cross = cross.group(1) if cross else "0"
    runs = open(d + "runs.jsonl").read() if os.path.exists(d + "runs.jsonl") else ""
    hp = re.findall(r'"hpwl_um":(\d+)', runs); hp = hp[0] if hp else "-"
    wall = re.search(r"wall_s=([\d.]+)", log); wall = f"{float(wall.group(1)):.1f}s" if wall else "-"
    judge = []
    for st in ["schematic", "placement", "routing"]:
        jp = d + f"judge_{st}.json"
        if os.path.exists(jp):
            v = json.load(open(jp)); judge.append((st, v))
    jcell = " / ".join(f"{st[:3]} {v['score']:.0f}" for st, v in judge) if judge else "no key"
    rows.append(f"<tr><td><a href='#{n}'>{n}</a></td><td>{parts}</td><td>{nets}</td>{gate(log,'schematic')}{gate(log,'placement')}{gate(log,'routing')}<td>{cross}</td><td>{hp}</td><td>{wall}</td><td class='{'ok' if judge else 'na'}'>{jcell}</td></tr>")
    st_html = ""
    if PREV and os.path.exists(f"{PREV}/{n}/schematic.svg"):
        st_html += f'<div class="stage"><h3>Schematic · previous round</h3><div class="sheet">{svg(f"{PREV}/{n}/schematic.svg")}</div></div>'
    if os.path.exists(d + "schematic.svg"):
        st_html += f'<div class="stage"><h3>Schematic</h3><div class="sheet">{svg(d + "schematic.svg")}</div></div>'
    for st in ["placement", "routing"]:
        p = d + f"judge_{st}.png"
        if os.path.exists(p): st_html += f'<div class="stage"><h3>{st.title()}</h3><div class="board">{png(p)}</div></div>'
    jh = ""
    for st, v in judge:
        defects = "".join(f"<li class='{d_['severity']}'>{html.escape(d_['check'])} @ {html.escape(d_.get('location',''))}: {html.escape(d_['hint'])}</li>" for d_ in v["defects"])
        jh += f"<div class='verdict'><b>Judge · {st} · {v['score']:.1f}/10</b> <span class='meta'>{html.escape(v['model'])} · {v['rubric_version']}</span><p>{html.escape(v['summary'])}</p><ul>{defects}</ul></div>"
    cards.append(f'''<section class="card" id="{n}"><header><h2>{n}</h2><span class="meta">{parts} parts · {nets} nets · {wall}</span></header>
<div class="stages">{st_html}</div>{jh}
{('<ul class="fails">' + ''.join(f'<li>{x}</li>' for x in fails) + '</ul>') if fails else ''}
<details><summary>{len(warns)} warnings</summary><ul class="warns">{''.join(f'<li>{w}</li>' for w in warns)}</ul></details></section>''')
title = os.path.basename(OUT.rstrip("/")) or "loop"
print(f'''<title>Loop {html.escape(title)}</title>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Sans:wght@400;600&family=IBM+Plex+Mono&display=swap">
<style>
:root{{--bg:#f6f5f1;--ink:#1d2430;--mute:#5d6470;--line:#d9d6cc;--card:#fff;--ok:#2f7d4f;--warn:#a86a11;--fail:#b3261e;--acc:#1f4e8c}}
@media (prefers-color-scheme:dark){{:root:not([data-theme="light"]){{--bg:#151820;--ink:#e6e4dc;--mute:#9aa0aa;--line:#2c313c;--card:#1c2029;--acc:#7aa7e0}}}}
:root[data-theme="dark"]{{--bg:#151820;--ink:#e6e4dc;--mute:#9aa0aa;--line:#2c313c;--card:#1c2029;--acc:#7aa7e0}}
body{{background:var(--bg);color:var(--ink);font:15px/1.5 "IBM Plex Sans",system-ui,sans-serif;margin:0;padding:32px 24px 64px}}
h1{{font-size:26px;margin:0 0 16px}} h2{{font-size:16px;margin:0;font-family:"IBM Plex Mono",monospace}} h3{{font-size:13px;margin:0 0 6px;color:var(--mute);font-weight:600;letter-spacing:.03em;text-transform:uppercase}}
table{{border-collapse:collapse;font-size:13px;margin-bottom:28px;font-variant-numeric:tabular-nums}} th,td{{border:1px solid var(--line);padding:5px 9px;text-align:left}} th{{background:var(--card)}} td a{{color:var(--acc);text-decoration:none}}
.ok{{color:var(--ok)}} .warn{{color:var(--warn)}} .fail{{color:var(--fail);font-weight:600}} .na{{color:var(--mute)}}
.card{{border:1px solid var(--line);background:var(--card);margin:0 0 28px}} .card header{{display:flex;gap:14px;align-items:center;padding:10px 16px;border-bottom:1px solid var(--line)}} .meta{{color:var(--mute);font-size:13px;margin-left:auto}}
.stages{{display:grid;grid-template-columns:repeat(auto-fit,minmax(360px,1fr));gap:16px;padding:16px}} .stage{{min-width:0}} .sheet{{background:#fff;border:1px solid var(--line);padding:8px;overflow:auto}} .board img{{width:100%;height:auto;display:block;border:1px solid var(--line)}}
.verdict{{padding:8px 16px;border-top:1px dashed var(--line);font-size:14px}} .verdict ul{{padding-left:18px;margin:4px 0}} .verdict li.fail{{color:var(--fail)}} .verdict li.warn{{color:var(--warn)}}
.fails{{margin:0;padding:8px 16px 8px 34px;font-size:13px;color:var(--fail)}} details{{padding:6px 16px 10px;font-size:13px;color:var(--mute)}} .warns{{margin:6px 0 0;padding-left:18px}} .wrap{{overflow-x:auto}}
</style>
<h1>Loop round: {html.escape(title)}</h1>
<div class="wrap"><table><tr><th>board</th><th>parts</th><th>nets</th><th>schematic</th><th>placement</th><th>routing</th><th>crossings</th><th>HPWL µm</th><th>wall</th><th>judge</th></tr>{''.join(rows)}</table></div>
{''.join(cards)}''')
