"""Feasibility probe: fit a procedural material recipe (brick / planks / cobbles + noise) to a
photoreal CC0 colour map, the way the planned importer would, and report fit metrics.

Usage: python3 -I fitprobe.py COLOR.png WIDTH_M HEIGHT_M KIND OUT_PREFIX
KIND: brick | planks | cobbles
"""
import json
import math
import sys

import numpy as np
from PIL import Image

RES = 128
rng = np.random.default_rng(1)


def s2l(c):
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def l2s(c):
    c = np.clip(c, 0, 1)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * c ** (1 / 2.4) - 0.055)


def lin_to_oklab(rgb):
    r, g, b = rgb[..., 0], rgb[..., 1], rgb[..., 2]
    l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b
    m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b
    s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b
    l, m, s = np.cbrt(l), np.cbrt(m), np.cbrt(s)
    return np.stack([
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ], -1)


def blur(img, sigma=2.0):
    k = int(3 * sigma) + 1
    x = np.arange(-k, k + 1)
    w = np.exp(-x * x / (2 * sigma * sigma))
    w /= w.sum()
    out = img
    for axis in (0, 1):
        out = np.apply_along_axis(lambda v: np.convolve(np.pad(v, k, mode="wrap"), w, mode="valid"), axis, out)
    return out


def load(path, wm, hm):
    im = Image.open(path).convert("RGB")
    # Resample to RES x RES (the texture covers wm x hm metres; keep the metric grid square:
    # use RES samples along the longer side).
    a = max(wm, hm)
    w, h = int(round(RES * wm / a)), int(round(RES * hm / a))
    im = im.resize((w, h), Image.BOX)
    lin = s2l(np.asarray(im, dtype=np.float64) / 255.0)
    return lin, a / RES  # metres per texel


def kmeans(x, k, iters=20):
    c = x[rng.choice(len(x), k, replace=False)]
    for _ in range(iters):
        d = ((x[:, None, :] - c[None]) ** 2).sum(-1)
        lab = d.argmin(1)
        for j in range(k):
            if (lab == j).any():
                c[j] = x[lab == j].mean(0)
    return c, np.bincount(lab, minlength=k) / len(x)


def radial_spectrum(L):
    """Power in 16 log-spaced radial bands x 4 orientations (normalised): shift invariant,
    keeps anisotropy (rows vs columns)."""
    f = np.abs(np.fft.fft2(L - L.mean())) ** 2
    h, w = L.shape
    fy = np.fft.fftfreq(h)[:, None] * np.ones((1, w))
    fx = np.fft.fftfreq(w)[None, :] * np.ones((h, 1))
    r = np.sqrt(fx * fx + fy * fy)
    ang = (np.arctan2(fy, fx) % np.pi) / np.pi  # 0..1
    rb = np.digitize(np.log2(np.maximum(r, 1e-6)), np.linspace(np.log2(1.0 / 128), np.log2(0.5), 17))
    ab = np.minimum((ang * 4).astype(int), 3)
    spec = np.zeros((18, 4))
    np.add.at(spec, (rb, ab), f)
    spec = spec[1:17]
    return (spec / max(spec.sum(), 1e-12)).ravel()


def axis_period(L, axis, mpt):
    # Dominant period (m) of the mean profile across one axis (rows -> brick row height).
    prof = L.mean(axis=1 - axis) if axis == 0 else L.mean(axis=0)
    prof = prof - prof.mean()
    f = np.abs(np.fft.rfft(prof)) ** 2
    f[0] = 0
    f[1] = 0
    k = int(f.argmax())
    return len(prof) / max(k, 1) * mpt


# ------------------------------------------------------------------ CPU kernels (numpy)

def pulse(d, w, fw):
    lo = np.maximum(d - 0.5 * fw, -0.5 * w)
    hi = np.minimum(d + 0.5 * fw, 0.5 * w)
    return np.clip((hi - lo) / fw, 0, 1)


def hash2(a, b, salt):
    h = (a.astype(np.int64) * 73856093 ^ b.astype(np.int64) * 19349663 ^ salt) & 0xFFFFFFFF
    h = (h * 747796405 + 2891336453) & 0xFFFFFFFF
    h = (((h >> ((h >> 28) + 4)) ^ h) * 277803737) & 0xFFFFFFFF
    return (((h >> 22) ^ h) >> 8) / 16777216.0


def vnoise2(x, y, salt):
    ix, iy = np.floor(x), np.floor(y)
    fx, fy = x - ix, y - iy
    ux, uy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
    a = hash2(ix, iy, salt)
    b = hash2(ix + 1, iy, salt)
    c = hash2(ix, iy + 1, salt)
    d = hash2(ix + 1, iy + 1, salt)
    return (a + (b - a) * ux) + ((c + (d - c) * ux) - (a + (b - a) * ux)) * uy


def fbm2(x, y, salt, oct=3):
    s, amp, n = 0, 0.5, 0
    for o in range(oct):
        s = s + amp * vnoise2(x, y, salt + 7 * o)
        n += amp
        amp *= 0.5
        x, y = x * 2.03, y * 2.03
    return s / n


def render(p, shape, mpt):
    h, w = shape
    v, u = np.mgrid[0:h, 0:w].astype(np.float64)
    u = (u + 0.5) * mpt + p["ox"]
    v = (v + 0.5) * mpt + p["oy"]
    fw = mpt
    base = np.array(p["base"])
    c = np.broadcast_to(base, (h, w, 3)).copy()
    kind = p["kind"]
    if kind in ("brick", "planks"):
        W, H, gap = p["W"], p["H"], p["gap"]
        row = np.floor(v / H)
        if kind == "planks":
            off = hash2(row, row * 0 + 7, 11) * W
        else:
            off = 0.5 * W * (row % 2)
        x = (u - off) / W
        col = np.floor(x)
        fu = x - col
        fv = v / H - row
        du = np.minimum(fu, 1 - fu) * W
        dv = np.minimum(fv, 1 - fv) * H
        d = np.minimum(du, dv)
        m = np.maximum(pulse(du, gap, fw), pulse(dv, gap, fw))
        r = hash2(col, row, 3)
    else:  # cobbles: 2D voronoi edges
        S, gap, jit = p["W"], p["gap"], p["jit"]
        q = np.stack([u / S, v / S], -1)
        ic = np.floor(q)
        d1 = np.full((h, w), 1e9)
        d2 = np.full((h, w), 1e9)
        bid = np.zeros((h, w, 2))
        for j in (-1, 0, 1):
            for i in (-1, 0, 1):
                cc = ic + np.array([i, j])
                rx = hash2(cc[..., 0], cc[..., 1], 5)
                ry = hash2(cc[..., 0], cc[..., 1], 9)
                fp = cc + 0.5 + jit * (np.stack([rx, ry], -1) - 0.5)
                dd = np.sqrt(((fp - q) ** 2).sum(-1))
                closer = dd < d1
                d2 = np.where(closer, d1, np.minimum(d2, dd))
                bid = np.where(closer[..., None], cc, bid)
                d1 = np.where(closer, dd, d1)
        d = 0.5 * (d2 - d1) * S
        m = pulse(d, gap, fw)
        r = hash2(bid[..., 0], bid[..., 1], 3)
    vary = (1 + p["vary"] * (2 * r - 1)) ** 2
    c = c * vary[..., None]
    prof = 1 - np.clip((d - gap * 0.5) / max(p["bevel"], 1e-4), 0, 1)
    prof = prof * prof * (3 - 2 * prof)
    s = 1 - p["cavity"] * prof
    c = c * (s * s)[..., None]
    gapc = np.array(p["gapc"])
    c = c + (gapc - c) * m[..., None]
    if p.get("noise", 0) > 0:
        n = fbm2(u / p["nsize"], v / p["nsize"], 21)
        nm = np.clip((n - 0.5) / 0.15 + 0.5, 0, 1)
        nc = np.array(p["ncol"])
        c = c + (nc - c) * (nm * p["noise"])[..., None]
    return c


# ------------------------------------------------------------------ metrics

def ssim(a, b):
    c1, c2 = 0.01 ** 2, 0.03 ** 2
    ma, mb = a.mean(), b.mean()
    va, vb = a.var(), b.var()
    cov = ((a - ma) * (b - mb)).mean()
    return ((2 * ma * mb + c1) * (2 * cov + c2)) / ((ma * ma + mb * mb + c1) * (va + vb + c2))


def local_ssim(a, b, win=8):
    h, w = a.shape
    vals = []
    for y in range(0, h - win + 1, win // 2):
        for x in range(0, w - win + 1, win // 2):
            vals.append(ssim(a[y:y + win, x:x + win], b[y:y + win, x:x + win]))
    return float(np.mean(vals))


def best_shift(a, b):
    # integer translation of b that best matches a (circular cross-correlation)
    fa = np.fft.fft2(a - a.mean())
    fb = np.fft.fft2(b - b.mean())
    cc = np.real(np.fft.ifft2(fa * np.conj(fb)))
    dy, dx = np.unravel_index(cc.argmax(), cc.shape)
    return np.roll(b, (dy, dx), axis=(0, 1))


def metrics(target_lab, fit_lin, spec_t):
    fit_lab = lin_to_oklab(fit_lin)
    tb = np.stack([blur(target_lab[..., i], 1.0) for i in range(3)], -1)
    fb = np.stack([blur(fit_lab[..., i], 1.0) for i in range(3)], -1)
    fb = np.stack([best_shift(tb[..., 0], fb[..., i]) if i == 0 else fb[..., i] for i in range(3)], -1)
    # colour: mean dE of the means + dE between sorted lightness quantiles (shift invariant)
    de_mean = float(np.linalg.norm(tb.reshape(-1, 3).mean(0) - fb.reshape(-1, 3).mean(0)) * 100)
    qt = np.quantile(tb[..., 0], np.linspace(0.05, 0.95, 19))
    qf = np.quantile(fb[..., 0], np.linspace(0.05, 0.95, 19))
    de_dist = float(np.abs(qt - qf).mean() * 100)
    spec_f = radial_spectrum(fit_lab[..., 0])
    spec = float(np.abs(spec_t - spec_f).sum() / 2)
    s = local_ssim(tb[..., 0], fb[..., 0])
    return {"de_mean": round(de_mean, 2), "de_dist": round(de_dist, 2), "spec": round(spec, 3), "ssim": round(s, 3)}


def loss(m):
    return 0.03 * m["de_mean"] + 0.04 * m["de_dist"] + 1.0 * m["spec"] + 1.0 * (1 - m["ssim"])


def to_hex(lin):
    s = l2s(np.array(lin))
    return "#%02x%02x%02x" % tuple(int(round(x * 255)) for x in s)


def main():
    path, wm, hm, kind, outp = sys.argv[1], float(sys.argv[2]), float(sys.argv[3]), sys.argv[4], sys.argv[5]
    lin, mpt = load(path, wm, hm)
    lab = lin_to_oklab(lin)
    spec_t = radial_spectrum(lab[..., 0])
    cents, cover = kmeans(lab.reshape(-1, 3), 3)
    order = np.argsort(-cover)
    # Oklab centroid -> linear rgb via nearest pixel colour (simple, avoids inverse transform)
    flat_lab = lab.reshape(-1, 3)
    flat_lin = lin.reshape(-1, 3)
    cols = [flat_lin[((flat_lab - cents[j]) ** 2).sum(1).argmin()] for j in order]
    Ls = [cents[j][0] for j in order]
    # Spectrum-seeded sizes
    py = axis_period(lab[..., 0], 0, mpt)
    px = axis_period(lab[..., 0], 1, mpt)
    print(f"texel {mpt*100:.2f} cm; palette (by coverage): {[to_hex(c) for c in cols]} cover {[round(float(cover[j]),2) for j in order]}; period x {px:.3f} m y {py:.3f} m")
    base = cols[0]
    gapc = cols[int(np.argmax(Ls))] if kind == "brick" else cols[int(np.argmin(Ls))]
    init = {"kind": kind, "W": px if kind != "cobbles" else max(px, py), "H": py, "gap": 0.02, "vary": 0.15, "cavity": 0.3,
            "bevel": 0.04, "base": list(base), "gapc": list(gapc), "jit": 0.8, "ox": 0.0, "oy": 0.0, "noise": 0.0,
            "nsize": 0.3, "ncol": list(cols[-1])}
    if kind == "planks":
        init["W"] = max(px * 4, 0.8)
    best, best_m, best_l = None, None, 1e9
    trials = int(sys.argv[6]) if len(sys.argv) > 6 else 160
    for t in range(trials):
        p = dict(init)
        if t > 0:
            src = best if best is not None and t > trials // 3 else init
            p = dict(src)
            sc = 0.5 if t <= trials // 3 else 0.15
            for k in ("W", "H"):
                p[k] = float(np.clip(src[k] * math.exp(rng.normal(0, sc * 0.4)), init[k] * 0.7, init[k] * 1.4))
            for k in ("gap", "bevel"):
                p[k] = max(0.005, src[k] * math.exp(rng.normal(0, sc)))
            for k in ("vary", "cavity", "noise", "jit"):
                p[k] = float(np.clip(src[k] + rng.normal(0, sc * 0.5), 0, 1))
            p["nsize"] = max(0.05, src["nsize"] * math.exp(rng.normal(0, sc)))
            for k in ("base", "gapc"):
                p[k] = list(np.clip(np.array(src[k]) * np.exp(rng.normal(0, sc * 0.3, 3)), 0, 1))
        img = render(p, lin.shape[:2], mpt)
        m = metrics(lab, img, spec_t)
        l = loss(m)
        if l < best_l:
            best, best_m, best_l = p, m, l
    img = render(best, lin.shape[:2], mpt)
    side = np.concatenate([l2s(lin), l2s(img)], axis=1)
    Image.fromarray((side * 255).astype(np.uint8)).resize((side.shape[1] * 2, side.shape[0] * 2), Image.NEAREST).save(outp + "_side.png")
    # Blurred (game-distance) comparison too
    bl = np.concatenate([blur(l2s(lin)[..., i], 2.0)[..., None] for i in range(3)], -1)
    bf = np.concatenate([blur(l2s(img)[..., i], 2.0)[..., None] for i in range(3)], -1)
    side = np.concatenate([bl, bf], axis=1)
    Image.fromarray((np.clip(side, 0, 1) * 255).astype(np.uint8)).resize((side.shape[1] * 2, side.shape[0] * 2), Image.NEAREST).save(outp + "_blur.png")
    pat = {"brick": "brick", "planks": "planks", "cobbles": "cobbles"}[kind]
    layer = {"type": pat, "size": [round(best["W"], 3), round(best["H"], 3)] if kind != "cobbles" else round(best["W"], 3),
             "gap": round(best["gap"], 3), "vary": round(best["vary"], 2), "cavity": round(best["cavity"], 2),
             "bevel": round(best["bevel"], 3), "color": to_hex(best["gapc"])}
    if kind == "cobbles":
        layer["jitter"] = round(best["jit"], 2)
    recipe = {"color": to_hex(best["base"]), "layers": [layer]}
    if best["noise"] > 0.05:
        recipe["layers"].append({"type": "noise", "size": round(best["nsize"], 2), "color": to_hex(best["ncol"]), "strength": round(best["noise"], 2)})
    init_m = metrics(lab, render(init, lin.shape[:2], mpt), spec_t)
    print("init metrics", init_m, "loss", round(loss(init_m), 3))
    print("best metrics", best_m, "loss", round(best_l, 3))
    # Baseline: flat mean colour
    flat = np.broadcast_to(lin.reshape(-1, 3).mean(0), lin.shape)
    fm = metrics(lab, flat, spec_t)
    print("flat-colour baseline", fm, "loss", round(loss(fm), 3))
    print("recipe", json.dumps(recipe, separators=(",", ":")))


main()
