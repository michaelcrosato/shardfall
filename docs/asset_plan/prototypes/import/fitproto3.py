# Prototype v3: fit once, then evaluate three policies per model and write F2-like JSON:
#   prims  = every component as its best primitive (no mesh fallback)
#   auto   = mesh fallback when the component's best local IoU < 0.85
#   mesh   = the whole model as one mesh part (exact geometry, per-face palette)
# Writes OUTDIR/<policy>/<name>.json (+ meshes/<name>.json) and prints IoU/dE/mesh%/chars.
# usage: python3 -I fitproto3.py OUTDIR GLB...
import sys, os, json, math
import numpy as np
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from gltfload import triangles
from structure import components
from fitproto import srgb_to_oklab, area, raster, view_matrix, VIEWS
import fitproto2 as F2

M2 = np.array([[0.2104542553, 0.7936177850, -0.0040720468], [1.9779984951, -2.4285922050, 0.4505937099], [0.0259040371, 0.7827717662, -0.8086757660]])
M1 = np.array([[0.4122214708, 0.5363325363, 0.0514459929], [0.2119034982, 0.6806995451, 0.1073969566], [0.0883024619, 0.2817188376, 0.6299787005]])
def hexc(lab):
    lin = np.clip(((lab @ np.linalg.inv(M2).T) ** 3) @ np.linalg.inv(M1).T, 0, 1)
    s = np.where(lin <= 0.0031308, lin * 12.92, 1.055 * lin ** (1 / 2.4) - 0.055)
    return '#%02x%02x%02x' % tuple(int(round(x * 255)) for x in s)

def euler_yxz(R):
    # R = Ry(yaw) Rx(pitch) Rz(roll); returns degrees
    pitch = math.degrees(math.asin(max(-1, min(1, -R[1, 2]))))
    yaw = math.degrees(math.atan2(R[0, 2], R[2, 2]))
    roll = math.degrees(math.atan2(R[1, 0], R[1, 1]))
    return yaw, pitch, roll

def cm(v):
    x = round(float(v) * 100, 1)
    return int(x) if abs(x - round(x)) < 0.05 else x

def part_json(f, col):
    k = f['kind']; par = f['par']; e = {}
    yaw, pitch, roll = euler_yxz(f['R'])
    if k in ('box', 'ellipsoid'):
        e = {'type': k, 'size': [cm(2 * x) for x in f['h']]}
    elif k == 'cylinder':
        e = {'type': 'cylinder', 'radius': cm(par['radius']), 'height': cm(par['height'])}
    elif k == 'cone':
        e = {'type': 'cone', 'radius': cm(par['radius']), 'radius_end': cm(par['radius_end']), 'height': cm(par['height'])}
    elif k == 'lathe':
        e = {'type': 'lathe', 'profile': [[cm(r), cm(y)] for r, y in par['profile']]}
    elif k == 'extrude':
        e = {'type': 'extrude', 'points': None, 'depth': cm(par['depth'])}
    if 'axis' in par and par['axis'] != (2 if k == 'extrude' else 1):
        e['axis'] = 'xyz'[par['axis']]
    e['pos'] = [cm(x) for x in f['c']]
    for nm, v in (('yaw', yaw), ('pitch', pitch), ('roll', roll)):
        if abs(v) > 0.5:
            e[nm] = int(round(v))
    e['color'] = hexc(col)
    if k == 'extrude':
        e['points'] = [[cm(x), cm(y)] for x, y in par['poly']]
    return e

def mesh_json(name, t, L):
    P = np.round(t.reshape(-1, 3) * 1000).astype(np.int64)
    uniq, inv = np.unique(P, axis=0, return_inverse=True); inv = inv.reshape(-1, 3)
    cols = [hexc(c) for c in L]
    pal = sorted(set(cols)); pidx = {c: i for i, c in enumerate(pal)}
    f = []
    for (a, b, c), col in zip(inv, cols):
        f += [int(a), int(b), int(c), pidx[col]]
    return json.dumps({'format': 1, 'name': name, 'unit': 'mm', 'tris': len(t), 'budget': 4096, 'palette': pal,
                       'v': [int(x) for x in uniq.reshape(-1)], 'f': f}, separators=(',', ':'))

def fit_all(path):
    j, prims = triangles(path)
    T = np.concatenate([p['tris'] for p in prims]); C = np.concatenate([p['colours'] for p in prims])
    L = srgb_to_oklab(C); A = area(T)
    lab, _ = components(T)
    comps = []
    for k in range(lab.max() + 1):
        sel = lab == k; t = T[sel]
        col = (L[sel] * A[sel, None]).sum(0) / max(A[sel].sum(), 1e-12)
        f = F2.fit_component(t)
        comps.append(dict(t=t, L=L[sel], A=A[sel], col=col, f=f))
    return T, L, A, comps

def evaluate(T, L, FT, FC):
    ev = []
    for name, (yaw, pitch) in VIEWS.items():
        M = view_matrix(yaw, pitch)
        P = T.reshape(-1, 3) @ M.T
        lo, hi = P[:, :2].min(0), P[:, :2].max(0)
        if np.min(hi - lo) < 0.02 * np.max(hi - lo):
            continue
        pad = (hi - lo).max() * 0.05; lo, hi = lo - pad, hi + pad
        ms, cs = raster(T, L, M, lo, hi); mf, cf = raster(FT, FC, M, lo, hi)
        both = ms & mf
        ev.append(((ms & mf).sum() / max((ms | mf).sum(), 1), np.linalg.norm(cs[both] - cf[both], axis=1).mean() * 100 if both.any() else 0))
    return min(e[0] for e in ev), sum(e[0] for e in ev) / len(ev), sum(e[1] for e in ev) / len(ev)

if __name__ == '__main__':
    out = sys.argv[1]
    for pol in ('prims', 'auto', 'mesh'):
        os.makedirs(os.path.join(out, pol, 'meshes'), exist_ok=True)
    print('name policy iou_min iou_mean dE mesh% asset_chars mesh_chars parts', flush=True)
    for path in sys.argv[2:]:
        name = os.path.basename(path).split('.')[0]
        try:
            T, L, A, comps = fit_all(path)
        except Exception as e:
            print('ERR', name, repr(e), flush=True); continue
        for pol in ('prims', 'auto', 'mesh'):
            FT, FC, parts, meshes, fb = [], [], {}, [], 0.0
            if pol == 'mesh':
                FT, FC = [T], [L]; fb = A.sum()
                meshes.append(mesh_json(name, T, L)); parts['body'] = {'mesh': f'pack/{name}'}
            else:
                for i, c in enumerate(comps):
                    f = c['f']
                    if pol == 'auto' and f['iou'] < 0.85:
                        FT.append(c['t']); FC.append(c['L']); fb += c['A'].sum()
                        meshes.append(mesh_json(f'{name}_{i}', c['t'], c['L'])); parts[f'part{i}'] = {'mesh': f'pack/{name}_{i}'}
                        continue
                    w = f['tris'] @ f['R'].T + f['c']
                    FT.append(w); FC.append(np.tile(c['col'], (len(w), 1)))
                    if f['kind'] == 'extrude':
                        # recover the polygon from the extrude tris' cap (store separately)
                        f['par']['poly'] = f['par'].get('poly', [])
                    parts[f'part{i}'] = part_json(f, c['col'])
            iou_min, iou_mean, de = evaluate(T, L, np.concatenate(FT), np.concatenate(FC))
            js = json.dumps({'format': 2, 'name': name, 'unit': 'cm', 'parts': parts}, separators=(',', ':'))
            open(os.path.join(out, pol, name + '.json'), 'w').write(js)
            mc = 0
            for k, m in enumerate(meshes):
                open(os.path.join(out, pol, 'meshes', f'{name}_{k}.json'), 'w').write(m); mc += len(m)
            print(name, pol, round(iou_min, 3), round(iou_mean, 3), round(de, 1), round(100 * fb / A.sum(), 1), len(js), mc, len(parts), flush=True)
