# Structure census of low-poly glTF models: what a segmenter can use.
# usage: python3 -I structure.py DIR [pattern]
import sys, os, glob, collections
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from gltfload import triangles

def components(tris):
    """Connected components over welded (0.1 mm) positions. tris: T,3,3 -> labels (T,)"""
    P = np.round(tris.reshape(-1, 3) * 10000).astype(np.int64)
    _, inv = np.unique(P, axis=0, return_inverse=True)
    inv = inv.reshape(-1, 3)
    parent = list(range(inv.max() + 1 if len(inv) else 0))
    def find(a):
        while parent[a] != a:
            parent[a] = parent[parent[a]]; a = parent[a]
        return a
    for a, b, c in inv:
        ra, rb, rc = find(a), find(b), find(c)
        parent[rb] = ra; parent[find(rc)] = ra
    lab = np.array([find(a) for a, _, _ in inv])
    _, lab = np.unique(lab, return_inverse=True)
    return lab, inv

def mirror_fraction(verts, axis, tol_mm=1.0):
    v = np.round(verts * 1000).astype(np.int64)
    s = set(map(tuple, v))
    m = v.copy(); m[:, axis] = -m[:, axis]
    c = np.round(verts.mean(axis=0) * 1000).astype(np.int64)
    # mirror about the bbox centre plane, not only the origin
    lo, hi = v.min(axis=0), v.max(axis=0); mid2 = lo[axis] + hi[axis]
    m2 = v.copy(); m2[:, axis] = mid2 - v[:, axis]
    f0 = np.mean([tuple(x) in s for x in m]); f1 = np.mean([tuple(x) in s for x in m2])
    return max(f0, f1)

def shape_key(t):
    v = np.round(t.reshape(-1, 3) * 1000).astype(np.int64)
    v = np.unique(v, axis=0)
    v = v - v.min(axis=0)
    return hash(v.tobytes()), len(v)

if __name__ == '__main__':
    d = sys.argv[1]; pat = sys.argv[2] if len(sys.argv) > 2 else '*.glb'
    rows = []
    for f in sorted(glob.glob(os.path.join(d, pat))):
        try:
            j, prims = triangles(f)
        except Exception as e:
            print('ERR', os.path.basename(f), e); continue
        if not prims:
            continue
        T = np.concatenate([p['tris'] for p in prims]); C = np.concatenate([p['colours'] for p in prims])
        lab, _ = components(T)
        ncomp = lab.max() + 1
        # single-colour components
        single = 0; comp_cols = []
        for k in range(ncomp):
            cc = C[lab == k]
            u = np.unique(cc, axis=0)
            single += len(u) == 1; comp_cols.append(len(u))
        keys = collections.Counter(shape_key(T[lab == k])[0] for k in range(ncomp))
        repeated = sum(c for c in keys.values() if c > 1)
        verts = T.reshape(-1, 3)
        mx = mirror_fraction(verts, 0); mz = mirror_fraction(verts, 2)
        ext = verts.max(axis=0) - verts.min(axis=0)
        nodes = len(j.get('nodes', [])); meshes = sum(1 for n in j.get('nodes', []) if 'mesh' in n)
        rows.append(dict(f=os.path.basename(f), tris=len(T), nodes=nodes, mesh_nodes=meshes, prims=len(prims),
                         mats=len(set(p['material'] for p in prims)), colours=len(np.unique(C, axis=0)), comps=int(ncomp),
                         single_col_comps=int(single), repeated_comps=int(repeated), mirror_x=round(float(mx), 2),
                         mirror_z=round(float(mz), 2), size_cm=[int(round(e * 100)) for e in ext],
                         names=[p['node'] for p in prims][:4]))
    for r in rows:
        print(r)
    if rows:
        med = lambda k: sorted(r[k] for r in rows)[len(rows) // 2]
        print('\nfiles', len(rows), '| median tris', med('tris'), '| median nodes', med('nodes'), 'mesh nodes', med('mesh_nodes'),
              '| median prims', med('prims'), '| median colours', med('colours'), '| median components', med('comps'))
        tc = sum(r['comps'] for r in rows)
        print('components total', tc, '| single-colour', sum(r['single_col_comps'] for r in rows),
              '(%.0f%%)' % (100 * sum(r['single_col_comps'] for r in rows) / max(tc, 1)),
              '| in repeated groups', sum(r['repeated_comps'] for r in rows),
              '| models mirror-symmetric >=0.95 in x', sum(r['mirror_x'] >= 0.95 for r in rows), 'in z', sum(r['mirror_z'] >= 0.95 for r in rows))
