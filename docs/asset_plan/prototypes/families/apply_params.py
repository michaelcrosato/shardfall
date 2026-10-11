"""Scratch check of the proposed parametric-family rule (planning aid, not engine code).

params: {name: {range:[lo,hi], int?, set:{path: factor}}} or {choices:[...], set:{path:[value per choice]}}
A numeric target field = value x factor; the file's literals are the defaults (validated to agree).
presets: {name: {param: value}}. array.span = evenly spread copies (step = span/(count-1)).
chance: per resolved copy, kept iff hash(seed, resolved id) < chance.
Usage: apply_params.py FAMILY.f2.json OUT_DIR [preset|-] [seed] [k=v ...]
"""
import copy, json, sys, os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "../../format/exp"))
import f2compile  # noqa: E402


def walk(doc, path):
    """Return (container, key) for a dotted path; parts.<id> is implied for the first segment."""
    segs = path.split(".")
    node = doc["materials"] if segs[0] == "materials" else doc["parts"]
    if segs[0] == "materials":
        segs = segs[1:]
    for s in segs[:-1]:
        node = node[int(s)] if isinstance(node, list) else node[s]
    last = segs[-1]
    return node, (int(last) if isinstance(node, list) else last)


def defaults(doc):
    out, issues = {}, []
    for name, p in doc.get("params", {}).items():
        if "choices" in p:
            out[name] = p["choices"][0]
            for path, vals in p["set"].items():
                c, k = walk(doc, path)
                if c.get(k, vals[0]) != vals[0] if isinstance(c, dict) else c[k] != vals[0]:
                    issues.append(f"{name}: {path} literal differs from the first choice")
            continue
        vals = []
        for path, f in p["set"].items():
            c, k = walk(doc, path)
            vals.append(c[k] / f)
        d = vals[0]
        for (path, _), v in zip(p["set"].items(), vals):
            if abs(v - d) > max(0.5, 0.005 * abs(d)):
                issues.append(f"{name}: {path} implies default {v:.2f}, first target implies {d:.2f}")
        lo, hi = p["range"]
        if not lo <= d <= hi:
            issues.append(f"{name}: default {d} outside {p['range']}")
        out[name] = round(d) if p.get("int") else round(d, 2)
    return out, issues


def apply(doc, values):
    doc = copy.deepcopy(doc)
    for name, v in values.items():
        p = doc["params"][name]
        if "choices" in p:
            i = p["choices"].index(v)
            for path, vals in p["set"].items():
                c, k = walk(doc, path)
                c[k] = vals[i]
            continue
        lo, hi = p["range"]
        assert lo <= v <= hi, f"{name}={v} outside {p['range']}"
        for path, f in p["set"].items():
            c, k = walk(doc, path)
            x = v * f
            c[k] = round(x) if p.get("int") else round(x, 2)
    doc.pop("params", None)
    doc.pop("presets", None)
    return doc


def lower_span(node):
    for n in node.values():
        a = n.get("array")
        if a and "span" in a:
            c = a["count"]
            a["step"] = [s / (c - 1) if c > 1 else 0 for s in a.pop("span")]
        if "parts" in n:
            lower_span(n["parts"])


def chances(doc):
    out = {}
    def rec(node):
        for pid, n in node.items():
            if "chance" in n:
                out[pid] = n.pop("chance")
            if "parts" in n:
                rec(n["parts"])
    rec(doc["parts"])
    return out


if __name__ == "__main__":
    fam = json.load(open(sys.argv[1]))
    outdir, preset = sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else "-"
    seed = int(sys.argv[4]) if len(sys.argv) > 4 else 1
    base, issues = defaults(fam)
    print("defaults", base, "issues", issues)
    vals = dict(base)
    if preset != "-":
        vals.update(fam["presets"][preset])
    for kv in sys.argv[5:]:
        k, v = kv.split("=")
        vals[k] = json.loads(v) if v[0].isdigit() or v[0] == "-" else v
    doc = apply(fam, vals)
    ch = chances(doc)
    lower_span(doc["parts"])
    v1 = f2compile.Compiler(doc).run()
    for name in list(v1["parts"]):
        for pid, c in ch.items():
            if name.startswith(pid):
                if (f2compile.fnv(f"{seed}:{name}") % 1000) / 1000 >= c:
                    del v1["parts"][name]
    tag = preset if preset != "-" else "default"
    v1["name"] = f"{fam['name']}_{tag}"
    v1["description"] = f"{fam['name']} family, {tag}: {vals}"
    path = os.path.join(outdir, v1["name"] + ".json")
    json.dump(v1, open(path, "w"), separators=(",", ":"))
    print(path, len(v1["parts"]), "parts", vals)
