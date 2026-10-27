# Compare two summary.txt of tools/board-roundtrip/run.sh: total lines that differ, boards identical, boards better and worse.
import sys
def load(p):
    d={}
    for l in open(p):
        f=l.split()
        if f[0]=='OK': d[f[1]]=int(f[2])
        else: d[f[1]]=None
    return d
a,b=load(sys.argv[1]),load(sys.argv[2])
worse=[(k,a[k],b.get(k)) for k in a if a[k] is not None and b.get(k) is not None and b[k]>a[k]]
better=[(k,a[k],b.get(k)) for k in a if a[k] is not None and b.get(k) is not None and b[k]<a[k]]
fails=[k for k in b if b[k] is None]
print('total',sum(v for v in a.values() if v),'->',sum(v for v in b.values() if v))
print('identical', sum(1 for v in a.values() if v==0),'->',sum(1 for v in b.values() if v==0))
print('better',len(better),'worse',len(worse),'fails',fails)
for k,x,y in sorted(worse,key=lambda t:t[2]-t[1],reverse=True)[:30]: print('  WORSE',k,x,y)
