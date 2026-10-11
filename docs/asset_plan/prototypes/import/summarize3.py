# Summarize fitproto3 output: per policy medians of IoU/dE/mesh%, and o200k tokens of the asset JSON
# (what an agent reads) and of the mesh files (read-only, repo bytes).
# usage: venv/bin/python -I summarize3.py V3TXT OUTDIR
import sys, os, glob, statistics
import tiktoken
enc = tiktoken.get_encoding('o200k_base')
txt, out = sys.argv[1], sys.argv[2]
rows = {}
for line in open(txt):
    p = line.split()
    if len(p) != 9 or p[0] == 'name':
        continue
    name, pol = p[0], p[1]
    rows.setdefault(pol, []).append(dict(name=name, iou_min=float(p[2]), iou_mean=float(p[3]), de=float(p[4]), mesh=float(p[5])))
for pol, rs in rows.items():
    toks, mtoks = [], []
    for r in rs:
        a = open(os.path.join(out, pol, r['name'] + '.json')).read()
        r['tok'] = len(enc.encode(a)); toks.append(r['tok'])
        m = sum(len(enc.encode(open(f).read())) for f in glob.glob(os.path.join(out, pol, 'meshes', r['name'] + '_*.json')))
        r['mtok'] = m; mtoks.append(m)
    med = lambda k: statistics.median(r[k] for r in rs)
    n = len(rs)
    print(f"{pol:6s} n={n} iou_min med {med('iou_min'):.3f} | >=0.90 {sum(r['iou_min']>=0.9 for r in rs)}/{n} >=0.85 {sum(r['iou_min']>=0.85 for r in rs)}/{n} | iou_mean med {med('iou_mean'):.3f} | dE med {med('de'):.1f} (<=6: {sum(r['de']<=6 for r in rs)}/{n}) | mesh% mean {sum(r['mesh'] for r in rs)/n:.1f} | asset tokens med {med('tok'):.0f} max {max(toks)} | mesh-file tokens med {med('mtok'):.0f} total {sum(mtoks)}")
    fit = [r for r in rs if r['iou_min'] >= 0.85 and r['iou_mean'] >= 0.90 and r['de'] <= 6 and r['mesh'] <= 50]
    print(f"        'fit' tier (iou_min>=0.85, iou_mean>=0.90, dE<=6, mesh%<=50): {len(fit)}/{n}")
