# Structural form of a .kicad_pcb for diffing two of them (tools/board-roundtrip/run.sh): uuids, tstamps, versions and zone fills dropped; a footprint
# or zone child, a setup or layers entry, one sorted line each, prefixed with the footprint reference or the zone key. From the round-2 review r2-board-io.
import sys,re
def tok(s):
    i=0;n=len(s);out=[]
    while i<n:
        c=s[i]
        if c in ' \t\r\n': i+=1
        elif c in '()': out.append(c); i+=1
        elif c=='"':
            j=i+1;buf=''
            while s[j]!='"':
                if s[j]=='\\': buf+=s[j:j+2]; j+=2
                else: buf+=s[j]; j+=1
            out.append('"'+buf+'"'); i=j+1
        else:
            j=i
            while j<n and s[j] not in ' \t\r\n()': j+=1
            out.append(s[i:j]); i=j
    return out
def parse(t):
    st=[[]]
    for x in t:
        if x=='(': st.append([])
        elif x==')': l=st.pop(); st[-1].append(l)
        else: st[-1].append(x)
    return st[0][0]
DROP={'uuid','tstamp','generator','generator_version','version','date','filled_polygon','fill_segments'}
def clean(e):
    if isinstance(e,list):
        return [clean(x) for x in e if not (isinstance(x,list) and x and x[0] in DROP)]
    return e
def s(e):
    if isinstance(e,list): return '('+' '.join(s(x) for x in e)+')'
    return e
d=clean(parse(tok(open(sys.argv[1]).read())))
# emit top-level items; footprints: header line + one line per child (sorted) prefixed with ref
items=[]
for x in d[1:]:
    if isinstance(x,list) and x[0]=='footprint':
        ref='?'
        for c in x:
            if isinstance(c,list) and c[0]=='property' and len(c)>2 and c[1]=='"Reference"': ref=c[2]
        for c in x[1:]:
            items.append('FP '+ref+' '+s(c))
    elif isinstance(x,list) and x[0]=='zone':
        key=s([c for c in x if isinstance(c,list) and c[0] in ('net','net_name','layer','layers','name')])+s([c for c in x if isinstance(c,list) and c[0]=='polygon'])[:120]
        for c in x[1:]:
            if not(isinstance(c,list) and c[0] in ('net','net_name','layer','layers','name')): items.append('ZONE '+key+' '+s(c))
    elif isinstance(x,list) and x[0] in ('setup','layers','general','net','title_block','paper','embedded_fonts'):
        if x[0] in ('layers','setup'):
            for c in x[1:]: items.append(x[0]+' '+s(c))
        else: items.append(s(x))
    else: items.append(s(x))
for l in sorted(items): print(l)
