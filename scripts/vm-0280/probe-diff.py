"""Krunk 0.28.0: compare two probe reports (a = Krunk, b = Chrome): every field but the surface,
then the feature surface (globals and interface members) both ways."""
import json, sys
a, b = (json.load(open(p)) for p in sys.argv[1:3])
def flat(o, p=''):
    if isinstance(o, dict):
        for k, v in o.items():
            yield from flat(v, f'{p}.{k}' if p else k)
    else:
        yield p, json.dumps(o)[:200]
fa, fb = dict(flat({k: v for k, v in a.items() if k != 'surface'})), dict(flat({k: v for k, v in b.items() if k != 'surface'}))
print('== differing fields (A | B)')
for k in sorted(set(fa) | set(fb)):
    if k.split('.')[0] in ('tag', 'when', 'origin') or k == 'ownCount':
        continue
    if fa.get(k) != fb.get(k):
        print(f'{k}: {fa.get(k)} | {fb.get(k)}')
sa, sb = a['surface'], b['surface']
print('== globals only in A:', sorted(set(sa) - set(sb)))
print('== globals only in B:', sorted(set(sb) - set(sa)))
for k in sorted(set(sa) & set(sb)):
    if sa[k] != sb[k] and ',' in (sa[k] + sb[k]):
        x, y = set(sa[k].split(',')), set(sb[k].split(','))
        print(f'== {k}: only A {sorted(x - y)} only B {sorted(y - x)}')
