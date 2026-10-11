"""Prototype of `asset_edit action=check` geometry lint for format-1 props (resolved parts).
OBB per part, 15-axis SAT with signed gap; thresholds as in the plan. Prints the compact report
(lines 'code: message') and its o200k token cost."""
import json, math, sys, itertools
import tiktoken
enc = tiktoken.get_encoding('o200k_base')

def quat_yxz(yaw, pitch, roll):
    # glam Quat::from_euler(YXZ, a, b, c) = Ry(a) * Rx(b) * Rz(c)
    def ry(a): c, s = math.cos(a), math.sin(a); return [[c,0,s],[0,1,0],[-s,0,c]]
    def rx(a): c, s = math.cos(a), math.sin(a); return [[1,0,0],[0,c,-s],[0,s,c]]
    def rz(a): c, s = math.cos(a), math.sin(a); return [[c,-s,0],[s,c,0],[0,0,1]]
    def mm(A, B): return [[sum(A[i][k]*B[k][j] for k in range(3)) for j in range(3)] for i in range(3)]
    return mm(mm(ry(math.radians(yaw)), rx(math.radians(pitch))), rz(math.radians(roll)))

def dot(a, b): return sum(x*y for x, y in zip(a, b))
def sub(a, b): return [x-y for x, y in zip(a, b)]
def add(a, b): return [x+y for x, y in zip(a, b)]
def mul(a, s): return [x*s for x in a]
def cross(a, b): return [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]
def norm(a): return math.sqrt(dot(a, a))

class Part:
    def __init__(self, name, p):
        self.name = name
        sh = p['shape']; t = sh['type']; self.type = t
        if t in ('box', 'rounded_box'): h = sh['half']
        elif t == 'sphere': h = [sh['radius']]*3
        elif t == 'capsule': h = [sh['radius'], sh['half_height']+sh['radius'], sh['radius']]
        elif t == 'cylinder': h = [sh['radius'], sh['half_height'], sh['radius']]
        self.h = h
        self.c = p.get('pos', [0, 0, 0])
        R = quat_yxz(p.get('yaw', 0), p.get('pitch', 0), p.get('roll', 0))
        self.ax = [[R[0][k], R[1][k], R[2][k]] for k in range(3)]  # columns = local axes in asset space
        self.color = p.get('color', '#e8704a'); self.look = p.get('look', 'cel')
        self.solid = p.get('solid', True); self.p = p
    def radius_on(self, n):
        return sum(abs(dot(self.ax[k], n))*self.h[k] for k in range(3))
    def interval(self, n):
        c = dot(self.c, n); r = self.radius_on(n); return c-r, c+r
    def corners(self):
        for sx, sy, sz in itertools.product((-1, 1), repeat=3):
            yield add(self.c, add(add(mul(self.ax[0], sx*self.h[0]), mul(self.ax[1], sy*self.h[1])), mul(self.ax[2], sz*self.h[2])))
    def aabb(self):
        e = [self.radius_on([1,0,0]), self.radius_on([0,1,0]), self.radius_on([0,0,1])]
        return sub(self.c, e), add(self.c, e)


def contains(p, q):
    d = sub(q, p.c); l = [dot(d, p.ax[m]) for m in range(3)]
    if p.type == 'sphere': return norm(d) <= p.h[0]
    if p.type == 'capsule':
        r = p.h[0]; hh = p.h[1] - r; y = max(-hh, min(hh, l[1]))
        return math.sqrt(l[0]**2 + (l[1]-y)**2 + l[2]**2) <= r
    if p.type == 'cylinder': return math.hypot(l[0], l[2]) <= p.h[0] and abs(l[1]) <= p.h[1]
    return all(abs(l[m]) <= p.h[m] for m in range(3))

def gap(a, b):
    """signed separation along the best SAT axis: >0 gap, <0 penetration depth (approx)."""
    axes = a.ax + b.ax + [cross(u, v) for u in a.ax for v in b.ax]
    best = -1e9
    for n in axes:
        l = norm(n)
        if l < 1e-6: continue
        n = mul(n, 1/l)
        a0, a1 = a.interval(n); b0, b1 = b.interval(n)
        s = max(b0 - a1, a0 - b1)
        best = max(best, s)
    return best

def lint(asset, target=None, wall=False):
    parts = [Part(k, v) for k, v in sorted(asset['parts'].items())]
    out = []
    TOL = 0.005
    n = len(parts)
    G = [[None]*n for _ in range(n)]
    for i in range(n):
        for j in range(i+1, n):
            G[i][j] = G[j][i] = gap(parts[i], parts[j])
    # ---- floor / grounding
    lo = min(p.aabb()[0][1] for p in parts); hi = max(p.aabb()[1][1] for p in parts)
    ground = [p.aabb()[0][1] <= lo + TOL for p in parts]
    if not wall:
        if lo < -TOL:
            names = [p.name for p in parts if p.aabb()[0][1] < -TOL]
            out.append(f"below_floor: {', '.join(names[:4])}{' +%d' % (len(names)-4) if len(names) > 4 else ''} reach y={lo*100:.0f} cm")
        elif lo > TOL:
            out.append(f"not_grounded: lowest part at y={lo*100:.0f} cm")
    # ---- components (floating)
    parent = list(range(n))
    def find(x):
        while parent[x] != x: parent[x] = parent[parent[x]]; x = parent[x]
        return x
    for i in range(n):
        for j in range(i+1, n):
            if G[i][j] <= TOL: parent[find(i)] = find(j)
    comps = {}
    for i in range(n): comps.setdefault(find(i), []).append(i)
    if len(comps) > 1:
        # main = component touching the floor (largest if several)
        def key(c): return (any(ground[i] for i in c), len(c))
        main = max(comps.values(), key=key)
        for c in comps.values():
            if c is main: continue
            g = min(G[i][j] for i in c for j in main)
            names = [parts[i].name for i in c]
            out.append(f"floating: {', '.join(names[:3])}{' +%d' % (len(names)-3) if len(names) > 3 else ''} ({g*100:.0f} cm from the rest)")
    # ---- through / hidden
    for i in range(n):
        for j in range(n):
            if i == j: continue
            a, b = parts[i], parts[j]
            if G[i][j] > -TOL: continue          # need real penetration
            # hidden: every corner of a inside b (box-like b only)
            if b.type in ('box', 'rounded_box') and all(all(abs(dot(sub(q, b.c), b.ax[k])) <= b.h[k] - 0.001 for k in range(3)) for q in a.corners()):
                out.append(f"hidden: {a.name} is inside {b.name}")
                continue
            if b.type not in ('box', 'rounded_box'): continue
            for k in range(3):
                nrm = b.ax[k]
                b0, b1 = b.interval(nrm); a0, a1 = a.interval(nrm)
                if a0 < b0 - TOL and a1 > b1 + TOL:
                    o = [m for m in range(3) if m != k]
                    inside = all(abs(dot(sub(a.c, b.c), b.ax[m])) <= b.h[m] for m in o)
                    lo_out, hi_out = b0 - a0, a1 - b1
                    small, big = min(lo_out, hi_out), max(lo_out, hi_out)
                    # poke: a crosses b and shows a short stub (0.5-3 cm) on one side only
                    if inside and small < 0.03 and big >= 0.03:
                        out.append(f"poke: {a.name} pokes {small*100:.1f} cm out of {b.name}")
    # ---- z-fighting: coplanar same-facing faces of box-like parts, different colour/look
    def faces(p):
        if p.type not in ('box', 'rounded_box', 'cylinder'): return
        for k in range(3):
            if p.type == 'cylinder' and k != 1: continue
            for s in (-1, 1):
                nrm = mul(p.ax[k], s)
                yield nrm, dot(add(p.c, mul(p.ax[k], s*p.h[k])), nrm), k
    for i in range(n):
        for j in range(i+1, n):
            a, b = parts[i], parts[j]
            if a.color == b.color and a.look == b.look: continue
            if G[i][j] > TOL: continue
            hit = None
            for na, da, ka in faces(a):
                for nb, db, kb in faces(b):
                    if dot(na, nb) > 0.999 and abs(da - db) < 0.001 and not (na[1] < -0.999 and abs(da + lo) < 0.001):
                        # 2D overlap of the two face rectangles (SAT in the plane)
                        others_a = [m for m in range(3) if m != ka]; others_b = [m for m in range(3) if m != kb]
                        axes2 = [a.ax[m] for m in others_a] + [b.ax[m] for m in others_b]
                        ok = True
                        for ax2 in axes2:
                            ia = (dot(a.c, ax2) - sum(abs(dot(a.ax[m], ax2))*a.h[m] for m in others_a), dot(a.c, ax2) + sum(abs(dot(a.ax[m], ax2))*a.h[m] for m in others_a))
                            ib = (dot(b.c, ax2) - sum(abs(dot(b.ax[m], ax2))*b.h[m] for m in others_b), dot(b.c, ax2) + sum(abs(dot(b.ax[m], ax2))*b.h[m] for m in others_b))
                            if min(ia[1], ib[1]) - max(ia[0], ib[0]) < 0.01: ok = False; break
                        if ok:
                            # sample a 3x3 grid of the overlap (in a's face frame), 2 mm outside the face;
                            # z-fighting shows where no third part covers it (>= 3 uncovered samples)
                            u, v = (a.ax[m] for m in others_a)
                            iu = [max(dot(a.c,u)-a.radius_on(u)+abs(dot(na,u))*0, dot(b.c,u)-sum(abs(dot(b.ax[m],u))*b.h[m] for m in others_b)),
                                  min(dot(a.c,u)+sum(abs(dot(a.ax[m],u))*a.h[m] for m in others_a), dot(b.c,u)+sum(abs(dot(b.ax[m],u))*b.h[m] for m in others_b))]
                            iu[0] = max(dot(a.c,u)-sum(abs(dot(a.ax[m],u))*a.h[m] for m in others_a), iu[0])
                            iv = [max(dot(a.c,v)-sum(abs(dot(a.ax[m],v))*a.h[m] for m in others_a), dot(b.c,v)-sum(abs(dot(b.ax[m],v))*b.h[m] for m in others_b)),
                                  min(dot(a.c,v)+sum(abs(dot(a.ax[m],v))*a.h[m] for m in others_a), dot(b.c,v)+sum(abs(dot(b.ax[m],v))*b.h[m] for m in others_b))]
                            open_n = 0
                            for fu in (0.17, 0.5, 0.83):
                                for fv in (0.17, 0.5, 0.83):
                                    su = iu[0] + (iu[1]-iu[0])*fu; sv = iv[0] + (iv[1]-iv[0])*fv
                                    q = add(add(mul(u, su), mul(v, sv)), mul(na, da + 0.002))
                                    if not any(contains(p3, q) for p3 in parts if p3 is not a and p3 is not b):
                                        open_n += 1
                            if open_n >= 3: hit = (na, da)
            if hit:
                axis = 'xyz'[max(range(3), key=lambda m: abs(hit[0][m]))]
                out.append(f"z_fight: {a.name} and {b.name} share a face ({axis}={hit[1]*100:.0f} cm)")
    # ---- mirror pairs by name
    names = {p.name: p for p in parts}
    swap = {'left': 'right', 'right': 'left', 'l': 'r', 'r': 'l'}
    seen = set()
    for p in parts:
        toks = p.name.replace('-', '_').split('_')
        alt = [swap.get(t, t) for t in toks]
        if alt == toks: continue
        q = names.get('_'.join(alt))
        if not q or (q.name, p.name) in seen: continue
        seen.add((p.name, q.name))
        bad = []
        if abs(p.c[0] + q.c[0]) > 0.01: bad.append(f"x {p.c[0]:+.2f}/{q.c[0]:+.2f}")
        if abs(p.c[1] - q.c[1]) > 0.01 or abs(p.c[2] - q.c[2]) > 0.01: bad.append("y/z differ")
        if any(abs(x - y) > 0.001 for x, y in zip(p.h, q.h)): bad.append("size differs")
        if p.color != q.color: bad.append("colour differs")
        if abs(p.p.get('yaw', 0) + q.p.get('yaw', 0)) > 1 or abs(p.p.get('roll', 0) + q.p.get('roll', 0)) > 1 or abs(p.p.get('pitch', 0) - q.p.get('pitch', 0)) > 1: bad.append("rotation not mirrored")
        if bad: out.append(f"mirror: {p.name}/{q.name} {', '.join(bad)}")
    # ---- size vs target
    mn = [min(p.aabb()[0][k] for p in parts) for k in range(3)]; mx = [max(p.aabb()[1][k] for p in parts) for k in range(3)]
    size = [round((mx[k]-mn[k])*100) for k in range(3)]
    if target:
        d = []
        for k, t in enumerate(target):
            if t is None: continue
            r = size[k] / t - 1
            if abs(r) > 0.1: d.append(f"{'xyz'[k]} {r*100:+.0f}%")
        if d: out.append(f"size: {'x'.join(map(str, size))} cm vs target {'x'.join('-' if t is None else str(t) for t in target)}: {', '.join(d)}")
    return out, size

if __name__ == '__main__':
    W = '<scratch>'
    files = {
        'bench': '/home/user/shardfall/assets/props/templates/bench.json',
        'crate': '/home/user/shardfall/assets/props/templates/crate.json',
        'lantern': '/home/user/shardfall/assets/props/templates/lantern.json',
        'box': '/home/user/shardfall/assets/props/templates/box.json',
        'market_stall': W + '/dogfood/props/workshop/market_stall.json',
        'well': W + '/dogfood/props/workshop/well.json',
    }
    targets = {'bench': [180, 90, 60], 'well': [200, 250, 200]}
    total = {}
    for k, f in files.items():
        a = json.load(open(f))
        out, size = lint(a, targets.get(k))
        rep = {'check': out[:10]} if out else {'check': 'clean'}
        if len(out) > 10: rep['check_more'] = len(out) - 10
        s = json.dumps(rep, separators=(',', ':'))
        print(f"== {k}: {len(a['parts'])} parts, size {size}, {len(out)} findings, {len(enc.encode(s))} tok")
        for line in out: print('   ', line)
