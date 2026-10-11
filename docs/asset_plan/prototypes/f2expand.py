#!/usr/bin/env python3
"""PLANNING REFERENCE (docs/ASSET_SYSTEM_PLAN.md). Anchors default to the plan's parent-frame rule.
Known difference: capsule from/to are treated as cap centres here; the plan (sec. 5.2) makes them
the visible ends, so the wall_torch flame in examples/format2 (already converted) resolves 2r shorter.

Independent prototype: expand a proposed format-2 prop into a valid format-1 prop.

Written from the format section's spec text (F.2-F.4) only, to cross-check its semantics.
Usage:
  python3 -I f2expand.py IN.f2.json [--name NAME] [--roles THEME] [--sockets]
                         [--frame spec|fix] [--anchors spec|fix] [--resolved OUT.json] > OUT.v1.json
--frame fix    : from/to frame uses +z made perpendicular when the axis is within 45 deg of vertical
--anchors fix  : top/bottom/front/back/left/right = support point of the target in the parent frame
--resolved     : also write the resolved (pre-lowering) parts with positions in doc units
Notes about approximations and ambiguities go to stderr.
"""
import json, math, sys

UNITS = {"m": 1.0, "cm": 0.01, "mm": 0.001}
DEFAULT_ROLES = {"$floor": "#7a7268", "$wall": "#9a948a", "$pillar": "#a89f92",
                 "$accent": "#c8a24a", "$light": "#ffc870"}
NOTES = []


def note(msg):
    if msg not in NOTES:
        NOTES.append(msg)


# ---------------------------------------------------------------- linear algebra (rows)
I3 = ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0))


def mm(a, b):
    return tuple(tuple(sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)) for i in range(3))


def mv(a, v):
    return tuple(sum(a[i][k] * v[k] for k in range(3)) for i in range(3))


def tr(a):
    return tuple(tuple(a[j][i] for j in range(3)) for i in range(3))


def det(a):
    return (a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]))


def add(a, b):
    return tuple(x + y for x, y in zip(a, b))


def sub(a, b):
    return tuple(x - y for x, y in zip(a, b))


def mul(a, s):
    return tuple(x * s for x in a)


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def norm(a):
    n = math.sqrt(dot(a, a))
    return mul(a, 1.0 / n) if n > 1e-12 else (0.0, 0.0, 0.0)


def rx(d):
    c, s = math.cos(math.radians(d)), math.sin(math.radians(d))
    return ((1, 0, 0), (0, c, -s), (0, s, c))


def ry(d):
    c, s = math.cos(math.radians(d)), math.sin(math.radians(d))
    return ((c, 0, s), (0, 1, 0), (-s, 0, c))


def rz(d):
    c, s = math.cos(math.radians(d)), math.sin(math.radians(d))
    return ((c, -s, 0), (s, c, 0), (0, 0, 1))


def cols(x, y, z):
    return ((x[0], y[0], z[0]), (x[1], y[1], z[1]), (x[2], y[2], z[2]))


def col(a, j):
    return (a[0][j], a[1][j], a[2][j])


def ypr(node):
    """Spec F.2: YXZ order, R = Ry(yaw) Rx(pitch) Rz(roll) (glam EulerRot::YXZ, props.rs:119-121)."""
    return mm(ry(node.get("yaw", 0)), mm(rx(node.get("pitch", 0)), rz(node.get("roll", 0))))


def to_yxz(r):
    sp = max(-1.0, min(1.0, -r[1][2]))
    pitch = math.asin(sp)
    if math.cos(pitch) > 1e-6:
        yaw = math.atan2(r[0][2], r[2][2])
        roll = math.atan2(r[1][0], r[1][1])
    else:
        roll = 0.0
        yaw = math.atan2(-r[2][0], r[0][0])
    return math.degrees(yaw), math.degrees(pitch), math.degrees(roll)


AXES = {"x": (1, 0, 0), "y": (0, 1, 0), "z": (0, 0, 1), "-x": (-1, 0, 0), "-y": (0, -1, 0), "-z": (0, 0, -1)}
# F.2 gives y->x: rz(-90) and y->z: rx(90) and "..."; the other four are this prototype's guess.
BASE_Y = {"y": I3, "x": rz(-90), "z": rx(90), "-x": rz(90), "-z": rx(-90), "-y": rx(180)}
# extrude's own axis is z (F.2); F.2 lists no base rotations for it -> guesses (ambiguity A7)
BASE_Z = {"z": I3, "x": ry(90), "y": rx(-90), "-x": ry(-90), "-y": rx(90), "-z": ry(180)}


def frame_from_to(d, mode):
    """F.2: local +y runs from->to; local +z = asset +y made perpendicular (+z if vertical); x = y cross z."""
    y = norm(d)
    if mode == "fix" and abs(y[1]) >= math.cos(math.radians(45)):
        z = norm(sub((0, 0, 1), mul(y, y[2])))  # near-vertical: front stays front (continuous at vertical)
    elif abs(y[1]) > 1 - 1e-9:
        z = (0.0, 0.0, 1.0)
    else:
        z = norm(sub((0, 1, 0), mul(y, y[1])))
    return cols(cross(y, z), y, z)


# ---------------------------------------------------------------- document helpers
class Doc:
    def __init__(self, doc, args):
        self.doc = doc
        self.u = UNITS[doc.get("unit", "m")]
        self.mats = doc.get("materials", {})
        self.args = args
        self.roles = args.roles

    def L(self, v):  # length(s) in doc units -> metres
        if isinstance(v, (list, tuple)):
            return tuple(self.L(x) for x in v)
        return float(v) * self.u


def kind(node):
    if "parts" in node:
        return "group"
    for k in ("ref", "mesh"):
        if k in node:
            return k
    return node.get("type")


def shape_dims(D, node):
    """Shape parameters in metres plus the local box (centre offset, half extents) in the shape frame."""
    t = node["type"]
    s = {"type": t}
    ft = "from" in node
    if ft:
        a, b = D.L(node["from"]), D.L(node["to"])
        s["len"] = math.sqrt(dot(sub(b, a), sub(b, a)))
    if t in ("box", "rounded_box", "ellipsoid", "wedge"):
        if "half" in node:
            size = mul(D.L(node["half"]), 2)
        else:
            size = D.L(node["size"])
        if ft:
            if len(size) != 2:
                note("A4 from/to box with a 3-value size: prototype uses [0] as across and [-1] as thick")
            size = (size[0], s["len"], size[-1])
        s["size"] = tuple(size)
        if t == "rounded_box":
            s["radius"] = D.L(node["radius"])
        if t == "wedge":
            s["peak"] = node.get("peak", 0)
        s["box"] = ((0, 0, 0), mul(s["size"], 0.5))
    elif t == "sphere":
        r = D.L(node["radius"])
        s["radius"] = r
        s["box"] = ((0, 0, 0), (r, r, r))
    elif t in ("capsule", "cylinder", "cone", "tube"):
        r0 = D.L(node["radius"])
        r1 = D.L(node["radius_end"]) if "radius_end" in node else (0.0 if t == "cone" else r0)
        if ft:
            h = s["len"]
        elif "height" in node:
            h = D.L(node["height"])
        else:
            h = 2 * D.L(node["half_height"]) + (2 * r0 if t == "capsule" else 0)
        s.update(r0=r0, r1=r1, h=h, arc=node.get("arc", 360))
        rm = max(r0, r1)
        if t == "capsule":
            # F.2/F.3: from/to = cap centres (SdfInstance a/b); height = total tip to tip
            centres = h if ft else max(h - r0 - r1, 0.0)
            s["centres"] = centres
            s["box"] = ((0, (r1 - r0) / 2, 0), (rm, centres / 2 + (r0 + r1) / 2, rm))
        else:
            s["box"] = ((0, 0, 0), (rm, h / 2, rm))
        if t == "tube":
            s["thick"] = D.L(node["thickness"])
    elif t == "torus":
        R, r = D.L(node["radius"]), D.L(node["minor_radius"])
        s.update(R=R, r=r, arc=node.get("arc", 360))
        s["box"] = ((0, 0, 0), (R + r, r, R + r))
    elif t == "extrude":
        pts = [D.L(p) for p in node["points"]]
        s["points"] = pts
        s["depth"] = D.L(node["depth"])
        xs, ys = [p[0] for p in pts], [p[1] for p in pts]
        c = ((min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2, 0)
        s["box"] = (c, ((max(xs) - min(xs)) / 2, (max(ys) - min(ys)) / 2, s["depth"] / 2))
    elif t == "lathe":
        prof = [D.L(p) for p in node["profile"]]
        s["profile"] = prof
        rs, ys = [p[0] for p in prof], [p[1] for p in prof]
        rm = max(rs)
        s["box"] = ((0, (min(ys) + max(ys)) / 2, 0), (rm, (max(ys) - min(ys)) / 2, rm))
    else:
        raise SystemExit(f"unsupported type {t}")
    return s


# ---------------------------------------------------------------- anchors (attach)
WORDS = {"top": (0, 1, 0), "bottom": (0, -1, 0), "front": (0, 0, 1), "back": (0, 0, -1),
         "left": (-1, 0, 0), "right": (1, 0, 0), "center": (0, 0, 0)}


def anchor_uvw(word):
    if isinstance(word, list):
        return tuple(float(x) for x in word)
    v = (0.0, 0.0, 0.0)
    for w in word.split("_"):
        v = add(v, WORDS[w])
    return v


def local_point(s, uvw):
    c, h = s["box"]
    return (c[0] + uvw[0] * h[0], c[1] + uvw[1] * h[1], c[2] + uvw[2] * h[2])


def aabb_point(s, p, R, uvw):
    """Proposed fix: words = faces of the part's AABB in the parent frame (centre + uvw * half)."""
    c, h = s["box"]
    corners = [add(p, mv(R, (c[0] + i * h[0], c[1] + j * h[1], c[2] + k * h[2])))
               for i in (-1, 1) for j in (-1, 1) for k in (-1, 1)]
    lo = [min(q[a] for q in corners) for a in range(3)]
    hi = [max(q[a] for q in corners) for a in range(3)]
    return tuple((lo[a] + hi[a]) / 2 + uvw[a] * (hi[a] - lo[a]) / 2 for a in range(3))


# ---------------------------------------------------------------- resolver
class Resolved:
    def __init__(self, rid, node, t, A, levels, s):
        self.rid, self.node, self.t, self.A, self.levels, self.s = rid, node, t, A, levels, s


class Expander:
    def __init__(self, D):
        self.D = D
        self.out = []

    def base_rot(self, node, s):
        base = BASE_Z if node.get("type") == "extrude" else BASE_Y
        ax = node.get("axis", "z" if node.get("type") == "extrude" else "y")
        R0 = mm(ypr(node), base[ax])
        if s and s.get("arc", 360) < 360:
            default = "+z" if ax in ("y", "-y") else "+y"
            f = node.get("facing", default).lstrip("+")
            g = mv(tr(R0), AXES[f])  # facing expressed in the shape frame
            if abs(g[1]) > 1e-6:
                note(f"facing {f} is not perpendicular to the shape axis")
            R0 = mm(R0, ry(math.degrees(math.atan2(g[0], g[2]))))
        return R0

    def local(self, nid, node, sibs, memo, stack):
        """(pos, R, s) of a node in its parent frame, before replication/mirror."""
        if nid in memo:
            return memo[nid]
        if nid in stack:
            raise SystemExit(f"attach cycle at {nid}")
        stack.add(nid)
        s = shape_dims(self.D, node) if kind(node) not in ("group", "ref", "mesh") else None
        if "from" in node:
            a, b = self.D.L(node["from"]), self.D.L(node["to"])
            R = mm(frame_from_to(sub(b, a), self.D.args.frame), ry(node.get("spin", 0)))
            p = mul(add(a, b), 0.5)
        else:
            R = self.base_rot(node, s)
            p = self.D.L(node.get("pos", [0, 0, 0]))
        if "attach" in node:
            p = self.attach(nid, node, R, s, sibs, memo, stack)
        stack.discard(nid)
        memo[nid] = (p, R, s)
        return memo[nid]

    def attach(self, nid, node, R, s, sibs, memo, stack):
        at = node["attach"]
        spec = {"to": at} if isinstance(at, str) else dict(at)
        tid = spec["to"]
        if "[" in tid or "." in tid:
            raise SystemExit("prototype resolves sibling targets only")
        tnode = sibs[tid]
        tp, tR, ts = self.local(tid, tnode, sibs, memo, stack)
        if ts is None:
            note("A10 attach to a group: the group's anchor box is unspecified")
            raise SystemExit("group target")
        at_word = spec.get("at", "top")
        mode = self.D.args.anchors
        if at_word in ("from", "to"):
            sign = -1 if at_word == "from" else 1
            if "from" not in tnode:
                note("A9 at=from/to on a target placed without from/to: prototype uses its local -y/+y end")
            uvw = (0, sign, 0)
            point = add(tp, mv(tR, local_point(ts, uvw)))
            n_at = mv(tR, (0, sign, 0))
        elif mode == "fix" and isinstance(at_word, str):
            n_at = anchor_uvw(at_word)
            point = aabb_point(ts, tp, tR, n_at)
        else:
            uvw = anchor_uvw(at_word)
            point = add(tp, mv(tR, local_point(ts, uvw)))
            n_at = norm(mv(tR, uvw))
        # self anchor
        if "self" in spec:
            su = anchor_uvw(spec["self"])
            if mode == "fix" and isinstance(spec["self"], str):
                off = aabb_point(s, (0, 0, 0), R, su)
            else:
                off = mv(R, local_point(s, su))
        elif n_at == (0.0, 0.0, 0.0):
            off = mv(R, s["box"][0])
        else:
            want = mul(n_at, -1)  # "side facing the target" = most opposite outward normal
            if mode == "fix":
                off = aabb_point(s, (0, 0, 0), R, tuple(-x for x in n_at))
            else:
                best = max(WORDS.values(), key=lambda w: dot(mv(R, w), want) if w != (0, 0, 0) else -9)
                off = mv(R, local_point(s, best))
        offset = self.D.L(node.get("offset", [0, 0, 0]))
        return add(sub(point, off), offset)

    def copies(self, node, p, R):
        """Yield (suffix, pos, A, cycle_entry). Replication first (parent frame), then mirror."""
        reps = []
        cyc = None
        if "array" in node:
            a = node["array"]
            n, step = a["count"], self.D.L(a["step"])
            cyc = a.get("cycle")
            for i in range(n):
                reps.append((f"[{i}]", add(p, mul(step, i - (n - 1) / 2)), R, i))
        elif "grid" in node:
            g = node["grid"]
            (na, nb), (sa, sb) = g["count"], self.D.L(g["step"])
            plane = g.get("plane", "xz")
            ea, eb = AXES[plane[0]], AXES[plane[1]]
            cyc = g.get("cycle")
            for j in range(nb):
                for i in range(na):
                    k = j * na + i  # row-major guess: first plane axis varies fastest (ambiguity A3)
                    off = add(mul(ea, sa * (i - (na - 1) / 2)), mul(eb, sb * (j - (nb - 1) / 2)))
                    reps.append((f"[{k}]", add(p, off), R, k))
        elif "radial" in node:
            r = node["radial"]
            n, rad = r["count"], self.D.L(r["radius"])
            start, arc = r.get("start", 0), r.get("arc", 360)
            ax = r.get("axis", "y")
            cyc = r.get("cycle")
            if ax != "y":
                note("A5 radial axis x/z: angle-0 direction unspecified; prototype uses +y")
            for i in range(n):
                ang = start + i * (arc / n if arc >= 360 else arc / max(n - 1, 1))
                Rax = {"y": ry, "x": rx, "z": rz}[ax.lstrip("-")](ang)
                e0 = (0, 0, 1) if ax == "y" else (0, 1, 0)
                reps.append((f"[{i}]", add(p, mv(Rax, mul(e0, rad))), mm(Rax, R), i))
        else:
            reps.append(("", p, R, None))
        mir = node.get("mirror", [])
        mir = [mir] if isinstance(mir, str) else mir
        subsets = [[]]
        for m in mir:
            subsets += [s + [m] for s in subsets]
        for suffix, q, A, i in reps:
            entry = cyc[i % len(cyc)] if (cyc and i is not None) else None
            for sset in subsets:
                M = I3
                for m in sset:
                    M = mm(M, tuple(tuple((-1.0 if (r == c and "xyz"[r] == m) else (1.0 if r == c else 0.0))
                                          for c in range(3)) for r in range(3)))
                tag = ("~" + "".join(sset)) if sset else ""
                yield suffix + tag, mv(M, q), mm(M, A), entry

    def level(self, node, entry):
        lv = {k: node[k] for k in ("mat", "color", "look", "emissive", "solid") if k in node}
        if entry is not None:
            if entry.startswith("#") or entry.startswith("$"):
                lv["color"] = entry
                lv.pop("mat", None)
            else:
                lv["mat"] = entry
        return lv

    def walk(self, parts, t, A, levels, prefix):
        memo, stack = {}, set()
        for nid in sorted(parts):
            node = parts[nid]
            k = kind(node)
            if k in ("ref", "mesh"):
                note(f"{k} nodes are out of scope for this prototype")
                continue
            p, R, s = self.local(nid, node, parts, memo, stack)
            for suffix, q, Aq, entry in self.copies(node, p, R):
                wt, wA = add(t, mv(A, q)), mm(A, Aq)
                lv = [self.level(node, entry)] + levels
                rid = prefix + nid + suffix
                if k == "group":
                    self.walk(node["parts"], wt, wA, lv, rid + ".")
                else:
                    self.out.append(Resolved(rid, node, wt, wA, lv, s))


# ---------------------------------------------------------------- surfaces
def fnv(text):
    h = 0xcbf29ce484222325
    for b in text.encode():
        h = ((h ^ b) * 0x100000001b3) & 0xFFFFFFFFFFFFFFFF
    return h


def surface(D, r):
    out = {}
    pattern = None
    for field in ("color", "look", "emissive"):
        for lv in r.levels:
            if field in lv:
                out[field] = lv[field]
                break
            m = D.mats.get(lv.get("mat", ""), None) if lv.get("mat") else None
            if lv.get("mat") and m is None:
                raise SystemExit(f"unknown material {lv['mat']} on {r.rid}")
            if m and field in m:
                out[field] = m[field]
                break
    for lv in r.levels:
        if lv.get("mat"):
            pattern = D.mats[lv["mat"]].get("pattern")
            break
    solid = True
    for lv in r.levels:
        if "solid" in lv:
            solid = lv["solid"]
            break
    if "color" not in out:
        raise SystemExit(f"v2 part without a surface: {r.rid}")
    col_ = out["color"]
    if col_.startswith("$"):
        col_ = D.roles[col_]
    j = r.node.get("jitter", {}).get("color")
    if j:
        u = (fnv(r.rid) % 20001) / 10000.0 - 1.0
        rgb = [int(col_[i:i + 2], 16) for i in (1, 3, 5)]
        rgb = [max(0, min(255, round(c * (1 + j * u)))) for c in rgb]
        col_ = "#%02x%02x%02x" % tuple(rgb)
        note("A12 jitter.color: space/channel unspecified; prototype multiplies sRGB by 1+j*u, u=hash(id) in [-1,1]")
    return col_, out.get("look", "cel"), out.get("emissive", 0), solid, pattern


# ---------------------------------------------------------------- lowering to format-1 primitives
def P_box(half, pos=(0, 0, 0), R=I3):
    return ({"type": "box", "half": list(half)}, pos, R)


def P_cyl(r, hh, pos=(0, 0, 0), R=I3):
    return ({"type": "cylinder", "half_height": hh, "radius": r}, pos, R)


def P_cap(r, hh, pos=(0, 0, 0), R=I3):
    return ({"type": "capsule", "half_height": hh, "radius": r}, pos, R)


def P_sph(r, pos=(0, 0, 0)):
    return ({"type": "sphere", "radius": r}, pos, I3)


def along(d):
    """Rotation taking local +y to direction d."""
    return frame_from_to(d, "spec")


def lower_one(r):
    """One format-1 primitive per resolved part (token baseline): bounding stand-ins for new shapes."""
    s, t = r.s, r.s["type"]
    if t in ("box", "rounded_box", "sphere"):
        return lower(r)
    if t == "ellipsoid" or (t == "capsule" and abs(s["r0"] - s["r1"]) < 1e-6):
        return lower(r)[:1] if t != "capsule" else [P_cap(s["r0"], s["centres"] / 2)]
    if t == "capsule":
        return [P_cap((s["r0"] + s["r1"]) / 2, s["centres"] / 2)]
    if t in ("cylinder", "cone", "tube"):
        return [P_cyl(max((s["r0"] + s["r1"]) / 2, 0.001), s["h"] / 2)]
    if t == "torus":
        return [P_cyl(s["R"] + s["r"], s["r"])]
    c, h = s["box"]
    return [P_box(h, c)]


def lower(r):
    s, t, node = r.s, r.s["type"], r.node
    pcs = []
    if t in ("box", "rounded_box"):
        half = mul(s["size"], 0.5)
        if t == "rounded_box":
            rad = min(s["radius"], min(half))
            pcs.append(({"type": "rounded_box", "half": list(half), "radius": rad}, (0, 0, 0), I3))
        else:
            pcs.append(P_box(half))
    elif t == "sphere":
        pcs.append(P_sph(s["radius"]))
    elif t == "ellipsoid":
        h = mul(s["size"], 0.5)
        k = max(range(3), key=lambda i: h[i])
        others = [h[i] for i in range(3) if i != k]
        rad = sum(others) / 2
        note("ellipsoid -> capsule along its longest axis (third-axis flattening lost)")
        if max(h) / min(h) < 1.15:
            pcs.append(P_sph(sum(h) / 3))
        else:
            R = along(AXES["xyz"[k]])
            pcs.append(P_cap(rad, max(h[k] - rad, 0.0), (0, 0, 0), R))
    elif t == "capsule":
        r0, r1, c = s["r0"], s["r1"], s["centres"]
        if abs(r0 - r1) < 1e-6:
            pcs.append(P_cap(r0, c / 2))
        else:
            n = max(3, min(10, math.ceil(c / (0.5 * min(r0, r1))) + 1))
            note("round cone (capsule radius_end) -> chain of spheres")
            for i in range(n):
                f = i / (n - 1)
                pcs.append(P_sph(r0 + f * (r1 - r0), (0, -c / 2 + f * c, 0)))
    elif t in ("cylinder", "cone"):
        r0, r1, h, arc = s["r0"], s["r1"], s["h"], s["arc"]
        if arc < 360:
            pcs += sector(max(r0, r1), h, arc)
            note(f"{t} arc {arc} -> stepped boxes")
        elif abs(r0 - r1) < 1e-6:
            pcs.append(P_cyl(r0, h / 2))
        else:
            n = 4 if t == "cone" else 3
            note(f"{t} with radius_end -> {n} stacked cylinders")
            for i in range(n):
                f = (i + 0.5) / n
                pcs.append(P_cyl(max(r0 + f * (r1 - r0), 0.001), h / n / 2, (0, -h / 2 + f * h, 0)))
    elif t == "tube":
        R, th, h, arc = s["r0"], s["thick"], s["h"], s["arc"]
        n = 16 if R >= 0.25 else 12
        n = max(2, round(n * arc / 360))
        d = math.radians(arc) / n
        chord = 2 * R * math.sin(d / 2) * 1.03
        for i in range(n):
            a = -math.radians(arc) / 2 + (i + 0.5) * d
            rm = R - th / 2
            pcs.append(P_box((chord / 2, h / 2, th / 2), (rm * math.sin(a), 0, rm * math.cos(a)), ry(math.degrees(a))))
        note("tube -> ring of boxes")
    elif t == "torus":
        R, rr, arc = s["R"], s["r"], s["arc"]
        n = max(3, round(12 * arc / 360))
        a0 = -math.radians(arc) / 2
        d = math.radians(arc) / n
        for i in range(n):
            pa = (R * math.sin(a0 + i * d), 0, R * math.cos(a0 + i * d))
            pb = (R * math.sin(a0 + (i + 1) * d), 0, R * math.cos(a0 + (i + 1) * d))
            seg = sub(pb, pa)
            ln = math.sqrt(dot(seg, seg))
            pcs.append(P_cap(rr, ln / 2, mul(add(pa, pb), 0.5), along(seg)))
        note("torus -> ring of capsules")
    elif t == "extrude":
        pcs += extrude_slices(s["points"], s["depth"])
        note("extrude -> horizontal slices (boxes)")
    elif t == "wedge":
        sx, sy, sz = s["size"]
        pk = s["peak"]
        n = 4
        for i in range(n):
            f = (i + 0.5) / n
            z0 = -sz / 2 + f * (pk * sz / 2 + sz / 2)
            z1 = sz / 2 - f * (sz / 2 - pk * sz / 2)
            pcs.append(P_box((sx / 2, sy / n / 2, max((z1 - z0) / 2, 0.0005)), (0, -sy / 2 + f * sy, (z0 + z1) / 2)))
        note("wedge -> stepped boxes")
    elif t == "lathe":
        prof = s["profile"]
        for (ra, ya), (rb, yb) in zip(prof, prof[1:]):
            if abs(yb - ya) > 1e-6:
                pcs.append(P_cyl(max((ra + rb) / 2, 0.001), abs(yb - ya) / 2, (0, (ya + yb) / 2, 0)))
        note("lathe -> stacked cylinders")
    return pcs


def sector(R, h, arc):
    """Cylinder sector centred on local +z; h along local y. Half discs use strips parallel to the diameter."""
    pcs = []
    if abs(arc - 180) < 1e-6:
        n = 4
        for i in range(n):
            z0, z1 = i * R / n, (i + 1) * R / n
            w = 2 * math.sqrt(max(R * R - ((z0 + z1) / 2) ** 2, 0))
            pcs.append(P_box((w / 2, h / 2, (z1 - z0) / 2), (0, 0, (z0 + z1) / 2)))
    else:
        n = max(2, math.ceil(arc / 30))
        d = math.radians(arc) / n
        for i in range(n):
            a = -math.radians(arc) / 2 + (i + 0.5) * d
            w = 2 * (R / 2) * math.tan(d / 2) * 2
            pcs.append(P_box((w / 2, h / 2, R / 2), (R / 2 * math.sin(a), 0, R / 2 * math.cos(a)), ry(math.degrees(a))))
    return pcs


def width_at(pts, y):
    xs = []
    n = len(pts)
    for i in range(n):
        (x0, y0), (x1, y1) = pts[i], pts[(i + 1) % n]
        if (y0 - y) * (y1 - y) <= 0 and abs(y1 - y0) > 1e-12:
            xs.append(x0 + (y - y0) * (x1 - x0) / (y1 - y0))
    return (min(xs), max(xs)) if xs else None


def extrude_slices(pts, depth):
    ys = sorted({p[1] for p in pts})
    wmax = max(p[0] for p in pts) - min(p[0] for p in pts)
    pcs = []
    for ya, yb in zip(ys, ys[1:]):
        wa = width_at(pts, ya + 1e-9 * (yb - ya)) or (0, 0)
        wb = width_at(pts, yb - 1e-9 * (yb - ya)) or (0, 0)
        dw = abs((wb[1] - wb[0]) - (wa[1] - wa[0]))
        n = max(1, min(6, math.ceil(dw / (0.2 * wmax))))
        for i in range(n):
            y0 = ya + (yb - ya) * i / n
            y1 = ya + (yb - ya) * (i + 1) / n
            w = width_at(pts, (y0 + y1) / 2)
            if not w:
                continue
            pcs.append(P_box(((w[1] - w[0]) / 2, (y1 - y0) / 2, depth / 2), ((w[0] + w[1]) / 2, (y0 + y1) / 2, 0)))
    return pcs


def pattern_pieces(r, pattern, D):
    """Brick rows -> thin mortar rings (prototype: rows run along the part's local y over its box)."""
    if pattern.get("type") != "brick" or r.s["type"] not in ("box", "rounded_box"):
        note(f"pattern {pattern.get('type')} on {r.rid}: dropped (flat colour)")
        return []
    rows = pattern.get("rows", 1)
    sx, sy, sz = r.s["size"]
    line = 0.008
    pcs = []
    for k in range(1, rows):
        y = -sy / 2 + k * sy / rows
        pcs.append(P_box((sx / 2 + 0.001, line / 2, sz / 2 + 0.001), (0, y, 0)))
    note("A13 pattern brick rows/cols: axis, unit and face mapping unspecified; prototype adds rows-1 mortar rings along local y")
    return pcs


# ---------------------------------------------------------------- output
Q = [4]


def rnd(x, q=None):
    q = Q[0] if q is None else q
    v = round(x, q)
    if v == 0:
        v = 0.0
    return int(v) if v == int(v) else v


def sanitize(rid):
    s = rid.replace("~", "_m").replace("[", "_").replace("]", "").replace(".", "_")
    return s[:60]


def emit(name, piece_shape, t, A, color, look, emissive, solid, parts):
    sh, pp, Rp = piece_shape
    pos = add(t, mv(A, pp))
    Rw = mm(A, Rp)
    if det(Rw) < 0:  # improper (mirrored) frame: primitives are symmetric in local x -> Rw * diag(-1,1,1)
        Rw = mm(Rw, ((-1, 0, 0), (0, 1, 0), (0, 0, 1)))
    yaw, pitch, roll = to_yxz(Rw)
    shape = {"type": sh["type"]}
    for k, v in sh.items():
        if k == "type":
            continue
        shape[k] = [rnd(x) for x in v] if isinstance(v, (list, tuple)) else rnd(v)
    part = {"shape": shape, "pos": [rnd(x) for x in pos]}
    for k, v in (("yaw", yaw), ("pitch", pitch), ("roll", roll)):
        if abs(rnd(v, 2)) > 0:
            part[k] = rnd(v, 2)
    part["color"] = color
    if look != "cel":
        part["look"] = look
    if emissive:
        part["emissive"] = emissive
    if not solid:
        part["solid"] = False
    if name in parts:
        raise SystemExit(f"duplicate part name {name}")
    parts[name] = part


def socket_frames(D, doc):
    out = {}
    for sid, sk in doc.get("sockets", {}).items():
        p = D.L(sk.get("pos", [0, 0, 0]))
        R = mm(ypr(sk), BASE_Y[sk.get("axis", "y")])
        out[sid] = (p, R)
    return out


def main():
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument("src")
    ap.add_argument("--name")
    ap.add_argument("--roles")
    ap.add_argument("--sockets", action="store_true")
    ap.add_argument("--frame", default="spec")
    ap.add_argument("--anchors", default="fix")
    ap.add_argument("--resolved")
    ap.add_argument("--one", action="store_true")
    ap.add_argument("--q", type=int, default=4)
    args = ap.parse_args()
    Q[0] = args.q
    doc = json.load(open(args.src))
    roles = dict(DEFAULT_ROLES)
    if args.roles:
        roles.update(theme_roles(args.roles))
    args.roles = roles
    D = Doc(doc, args)
    if doc.get("format") != 2 or doc.get("unit") not in UNITS:
        raise SystemExit("needs format 2 and a unit")
    X = Expander(D)
    root_level = {k: doc[k] for k in ("mat", "color", "look", "emissive", "solid") if k in doc}
    X.walk(doc["parts"], (0, 0, 0), I3, [root_level], "")
    parts = {}
    resolved = []
    for r in X.out:
        color, look, emissive, solid, pattern = surface(D, r)
        pcs = lower_one(r) if args.one else lower(r)
        if pattern and not args.one:
            pcs += [(pc, "mortar") for pc in pattern_pieces(r, pattern, D)]
        base = sanitize(r.rid)
        multi = len(pcs) > 1
        for i, pc in enumerate(pcs):
            if isinstance(pc[1], str):  # mortar line
                pc = pc[0]
                mcol = D.mats[[lv["mat"] for lv in r.levels if lv.get("mat")][0]]["pattern"].get("mortar", "#000000")
                emit(f"{base}_l{i}", pc, r.t, r.A, mcol, look, 0, False, parts)
            else:
                emit(f"{base}_p{i}" if multi else base, pc, r.t, r.A, color, look, emissive, solid, parts)
        if "light" in r.node:
            note(f"light on {r.rid} lost (format 1 only auto-lights emissive>0.5)")
        if "fx" in r.node:
            note(f"fx on {r.rid} lost")
        resolved.append({"id": r.rid, "type": r.s["type"], "pos": [round(x / D.u, 2) for x in r.t],
                         "ypr": [round(a, 2) for a in to_yxz(r.A if det(r.A) > 0 else mm(r.A, ((-1, 0, 0), (0, 1, 0), (0, 0, 1))))],
                         "mirrored": det(r.A) < 0, "color": color, "solid": solid})
    if args.sockets:
        for sid, (p, R) in socket_frames(D, doc).items():
            emit(f"socket_{sid}", P_sph(0.015), p, I3, "#ff00ff", "unlit", 0, False, parts)
            tip = mv(R, (0, 0.08, 0))
            emit(f"socket_{sid}_dir", P_cap(0.006, 0.04, mul(tip, 0.5), along(tip)), p, I3, "#ff00ff", "unlit", 0, False, parts)
    if len(parts) > 256:
        raise SystemExit(f"{len(parts)} format-1 parts > 256")
    name = args.name or doc["name"]
    out = {"format": 1, "name": name, "description": (doc.get("description", "") + " [format-2 prototype expansion]")[:4096],
           "parts": parts}
    lines = ['{"format":1,"name":%s,"description":%s,"parts":{' % (json.dumps(name), json.dumps(out["description"]))]
    items = [json.dumps(k) + ":" + json.dumps(v, separators=(",", ":")) for k, v in parts.items()]
    lines.append(",\n".join(items))
    lines.append("}}")
    sys.stdout.write("\n".join(lines) + "\n")
    if args.resolved:
        json.dump(resolved, open(args.resolved, "w"), indent=0)
    for n_ in NOTES:
        print("note:", n_, file=sys.stderr)
    print(f"resolved={len(X.out)} format1_parts={len(parts)}", file=sys.stderr)


def theme_roles(theme):
    roles, cur = {}, None
    for line in open("/home/user/shardfall/game/themes.toml"):
        line = line.strip()
        if line.startswith("["):
            cur = line.strip("[]")
        elif cur == theme and "=" in line:
            k, v = [x.strip() for x in line.split("=", 1)]
            if k in ("floor", "wall", "pillar", "accent", "light"):
                v = json.loads(v)
                roles["$" + k] = v[0] if isinstance(v, list) else v
    return roles


if __name__ == "__main__":
    main()
