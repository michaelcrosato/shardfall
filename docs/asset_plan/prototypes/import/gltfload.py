# Minimal glTF/GLB loader for measurement (untrusted input: read-only parsing, no eval).
# Returns world-space triangles with per-face base colour (sRGB 0-255) sampled from the
# base-colour texture as a UV-area mean (7 barycentric samples), or the material factor.
import json, struct, os, io
import numpy as np
from PIL import Image

CT = {5120: np.int8, 5121: np.uint8, 5122: np.int16, 5123: np.uint16, 5125: np.uint32, 5126: np.float32}
NC = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4, 'MAT4': 16}

def load(path):
    data = open(path, 'rb').read()
    base = os.path.dirname(path)
    if data[:4] == b'glTF':
        clen, _ = struct.unpack_from('<II', data, 12)
        j = json.loads(data[20:20 + clen]); off = 20 + clen
        bins = [b'']
        if off + 8 <= len(data):
            blen, btype = struct.unpack_from('<II', data, off)
            assert btype == 0x004E4942, 'second chunk is not BIN'
            bins = [data[off + 8:off + 8 + blen]]
    else:
        j = json.loads(data); bins = []
        for b in j.get('buffers', []):
            uri = b.get('uri', '')
            if uri.startswith('data:'):
                import base64
                bins.append(base64.b64decode(uri.split(',', 1)[1]))
            else:
                bins.append(open(os.path.join(base, uri), 'rb').read())
    return j, bins, base

def acc(j, bins, i):
    a = j['accessors'][i]
    n = NC[a['type']]; dt = CT[a['componentType']]
    if 'bufferView' not in a:
        return np.zeros((a['count'], n), np.float32)
    v = j['bufferViews'][a['bufferView']]
    b = bins[v.get('buffer', 0)] if v.get('buffer', 0) < len(bins) else bins[0]
    start = v.get('byteOffset', 0) + a.get('byteOffset', 0)
    stride = v.get('byteStride', 0); isz = np.dtype(dt).itemsize * n
    if stride and stride != isz:
        raw = np.frombuffer(b, dtype=np.uint8, count=stride * (a['count'] - 1) + isz, offset=start)
        rows = np.lib.stride_tricks.as_strided(raw, shape=(a['count'], isz), strides=(stride, 1))
        arr = np.frombuffer(rows.copy().tobytes(), dtype=dt).reshape(a['count'], n)
    else:
        arr = np.frombuffer(b, dtype=dt, count=a['count'] * n, offset=start).reshape(a['count'], n)
    if a.get('normalized') and dt != np.float32:
        arr = arr.astype(np.float32) / np.iinfo(dt).max
    return arr

def image(j, bins, base, ti):
    img = j['images'][j['textures'][ti]['source']]
    if 'bufferView' in img:
        v = j['bufferViews'][img['bufferView']]; s = v.get('byteOffset', 0)
        raw = bins[v.get('buffer', 0)][s:s + v['byteLength']]
    else:
        uri = img['uri']
        if uri.startswith('data:'):
            import base64
            raw = base64.b64decode(uri.split(',', 1)[1])
        else:
            raw = open(os.path.join(base, uri.replace('%20', ' ')), 'rb').read()
    return np.asarray(Image.open(io.BytesIO(raw)).convert('RGBA'))

def node_matrix(n):
    if 'matrix' in n:
        return np.array(n['matrix'], dtype=np.float64).reshape(4, 4).T
    t = n.get('translation', [0, 0, 0]); r = n.get('rotation', [0, 0, 0, 1]); s = n.get('scale', [1, 1, 1])
    x, y, z, w = r
    R = np.array([[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
                  [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                  [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]])
    L = np.eye(4); L[:3, :3] = R * np.array(s); L[:3, 3] = t
    return L

BARY = np.array([[1/3, 1/3, 1/3], [2/3, 1/6, 1/6], [1/6, 2/3, 1/6], [1/6, 1/6, 2/3],
                 [0.5, 0.5, 0], [0, 0.5, 0.5], [0.5, 0, 0.5]])

def triangles(path):
    """-> list of dict(node, mesh, prim, material, tris (T,3,3) world, colours (T,3) uint8)"""
    j, bins, base = load(path)
    out = []
    imgs = {}
    def walk(ni, M, depth=0):
        n = j['nodes'][ni]
        W = M @ node_matrix(n)
        if 'mesh' in n and 'skin' not in n:
            for pi, p in enumerate(j['meshes'][n['mesh']]['primitives']):
                if p.get('mode', 4) != 4:
                    continue
                pos = acc(j, bins, p['attributes']['POSITION']).astype(np.float64)
                pw = (np.c_[pos, np.ones(len(pos))] @ W.T)[:, :3]
                idx = acc(j, bins, p['indices']).reshape(-1).astype(np.int64) if 'indices' in p else np.arange(len(pos))
                tri = idx.reshape(-1, 3)
                if np.linalg.det(W[:3, :3]) < 0:
                    tri = tri[:, ::-1]
                mi = p.get('material')
                mat = j['materials'][mi] if mi is not None else {}
                pbr = mat.get('pbrMetallicRoughness', {})
                f = np.array(pbr.get('baseColorFactor', [1, 1, 1, 1]), dtype=np.float64)
                lin = np.tile(f[:3], (len(tri), 1))
                if 'baseColorTexture' in pbr and 'TEXCOORD_0' in p['attributes']:
                    ti = pbr['baseColorTexture']['index']
                    if ti not in imgs:
                        imgs[ti] = image(j, bins, base, ti)
                    im = imgs[ti]; H, Wd = im.shape[:2]
                    uv = acc(j, bins, p['attributes']['TEXCOORD_0']).astype(np.float64)
                    tuv = uv[tri]  # T,3,2
                    s = np.einsum('kc,tcd->tkd', BARY, tuv)  # T,7,2
                    px = np.mod(np.floor(np.mod(s[..., 0], 1.0) * Wd), Wd).astype(int)
                    py = np.mod(np.floor(np.mod(s[..., 1], 1.0) * H), H).astype(int)
                    c = im[py, px, :3].astype(np.float64) / 255.0  # sRGB
                    clin = np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)
                    lin = clin.mean(axis=1) * f[:3]
                if 'COLOR_0' in p['attributes']:
                    vc = acc(j, bins, p['attributes']['COLOR_0']).astype(np.float64)[:, :3]
                    lin = lin * vc[tri].mean(axis=1)
                srgb = np.where(lin <= 0.0031308, lin * 12.92, 1.055 * np.power(np.clip(lin, 0, None), 1 / 2.4) - 0.055)
                out.append(dict(node=n.get('name', str(ni)), mesh=j['meshes'][n['mesh']].get('name', ''), prim=pi,
                                material=mat.get('name', ''), tris=pw[tri], colours=np.clip(np.round(srgb * 255), 0, 255).astype(np.uint8)))
        for c in n.get('children', []):
            walk(c, W, depth + 1)
    scenes = j.get('scenes')
    roots = scenes[j.get('scene', 0)]['nodes'] if scenes else [i for i in range(len(j.get('nodes', [])))]
    for r in roots:
        walk(r, np.eye(4))
    return j, out
