#!/usr/bin/env python3
"""Search the JLC parts database: q.py [--cat TEXT] [--pkg TEXT] [--basic] [--min-stock N] [-n N] WORDS...
Words match the description, mfr part number or manufacturer (all must match). Sorted: basic first, then stock."""
import sqlite3, sys, json, argparse, os
a = argparse.ArgumentParser(); a.add_argument('words', nargs='*'); a.add_argument('--cat'); a.add_argument('--pkg')
a.add_argument('--basic', action='store_true'); a.add_argument('--min-stock', type=int, default=100); a.add_argument('-n', type=int, default=15)
o = a.parse_args()
c = sqlite3.connect(os.environ.get('JLCPARTS_DB', os.path.expanduser('~/src/jlcparts/cache.sqlite3')))
w, p = ["present=1", "stock>=?"], [o.min_stock]
for t in o.words: w.append("(description like ? or mfr like ? or manufacturer like ?)"); p += [f"%{t}%"] * 3
if o.cat: w.append("(category like ? or subcategory like ?)"); p += [f"%{o.cat}%"] * 2
if o.pkg: w.append("package like ?"); p += [f"%{o.pkg}%"]
if o.basic: w.append("library_type in ('base','basic')")
q = f"select lcsc, mfr, manufacturer, package, library_type, preferred, stock, price, description, datasheet from jlc_components where {' and '.join(w)} order by (library_type in ('base','basic')) desc, preferred desc, stock desc limit ?"
for r in c.execute(q, p + [o.n]):
    try: pr = r[7].split(',')[0].split(':')[1]
    except Exception: pr = None
    lib = 'BASIC' if r[4] in ('base', 'basic') else ('pref' if r[5] else 'ext')
    print(f"C{r[0]:<9} {lib:5} stock {r[6]:>8}  ${pr if pr is not None else '?':<8} {r[1][:28]:28} {r[2][:16]:16} {r[3][:18]:18} {r[8][:70]}")
