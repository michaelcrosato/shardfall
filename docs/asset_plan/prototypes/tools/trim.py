"""Prototype of Phase 0 reply hygiene on REAL pav replies (out/session1.txt).
Counts o200k tokens before/after each step. Mirrors the Rust rules in the plan."""
import json, struct, sys, math
import tiktoken
W = '<scratch>'
enc = tiktoken.get_encoding('o200k_base')
def tok(s): return len(enc.encode(s))
def dump(v): return json.dumps(v, separators=(',', ':'), sort_keys=True)  # serde_json Value = BTreeMap order

def f32_short(x):
    """shortest decimal that round-trips to the same f32 (what ryu/serde_json print for f32)."""
    f = struct.unpack('f', struct.pack('f', x))[0]
    for p in range(1, 18):
        s = '%.*g' % (p, f)
        if struct.unpack('f', struct.pack('f', float(s)))[0] == f:
            return float(s)
    return x

def is_f32(x):
    try:
        return struct.unpack('f', struct.pack('f', x))[0] == x
    except OverflowError:
        return False

def tidy(v):
    """Rule T1: every float that is an exact f32 is re-printed as its shortest f32 decimal;
    integral floats print as integers (1.0 -> 1). Genuine f64 values are untouched."""
    if isinstance(v, float):
        if is_f32(v):
            v = f32_short(v)
        return int(v) if v == int(v) and abs(v) < 1e15 else v
    if isinstance(v, list): return [tidy(x) for x in v]
    if isinstance(v, dict): return {k: tidy(x) for k, x in v.items()}
    return v

def r3(x): # computed lengths in metres: millimetres
    y = round(x, 3); return int(y) if y == int(y) else y

DEFAULTS = {'pos': [0, 0, 0], 'yaw': 0, 'pitch': 0, 'roll': 0, 'look': 'cel', 'emissive': 0, 'solid': True}
def strip_defaults(asset):
    a = dict(asset)
    if a.get('description', None) == '': a.pop('description')
    parts = {}
    for k, p in a['parts'].items():
        parts[k] = {f: x for f, x in p.items() if DEFAULTS.get(f, object()) != x}
    a['parts'] = parts
    return a

def size_cm(b):
    return 'x'.join(str(int(round((b['max'][i] - b['min'][i]) * 100))) for i in range(3))

def compact_bounds(b):
    return [[r3(x) for x in b['min']], [r3(x) for x in b['max']]]

lines = open(W + '/out/session1.txt').read().splitlines()
names = ['inspect_bench', 'inspect_lantern', 'assets', 'copy', 'patch', 'preview_status', 'spawn_list', 'studio_status']
real = dict(zip(names, [json.loads(l) for l in lines]))
rows = []
def rec(label, stage, v):
    s = v if isinstance(v, str) else dump(v)
    rows.append((label, stage, len(s), tok(s)))
    open(f'{W}/proto/{label}.{stage}.json', 'w').write(s + '\n')

# ---------- inspect ----------
for label in ['inspect_bench', 'inspect_lantern']:
    v = real[label]
    rec(label, '0_current', lines[names.index(label)])
    t = tidy(v); rec(label, '1_f32', t)
    t2 = dict(t); t2['asset'] = strip_defaults(t['asset']); rec(label, '2_nodefaults', t2)
    t3 = {'name': t['name'], 'revision': t['revision'], 'size_cm': size_cm(v['bounds']),
          'bounds': compact_bounds(v['bounds']), 'asset': strip_defaults(t['asset'])}
    for k in ('undo', 'redo'):
        if v[k]: t3[k] = v[k]
    rec(label, '3_trimmed', t3)
    # opt-in extras, for reference
    t4 = dict(t3); t4['part_bounds'] = {k: compact_bounds(b) for k, b in v['parts'].items()}
    rec(label, '4_with_bounds_opt_in', t4)

# ---------- assets ----------
v = real['assets']
rec('assets', '0_current', lines[names.index('assets')])
rec('assets', '1_f32', tidy(v))
cols = {'cols': 'name size_cm parts desc', 'total': v['total'], 'matched': v['matched'],
        'rows': [[a['name'], size_cm(a['bounds']), a['part_count'], a['description']] for a in v['assets']]}
rec('assets', '3_trimmed', cols)

# ---------- copy / patch ----------
for label in ['copy', 'patch']:
    v = real[label]
    rec(label, '0_current', lines[names.index(label)])
    rec(label, '1_f32', tidy(v))
    t = {'name': v['name'], 'revision': v['revision'], 'saved': v['saved'], 'size_cm': size_cm(v['bounds'])}
    if v['undo']: t['undo'] = v['undo']
    if v['redo']: t['redo'] = v['redo']
    if label == 'copy':
        t['parts'] = v['part_count']           # create/copy: count instead of listing every part
    else:
        t['changed'] = v['changed_parts']      # renamed from changed_parts (old 'changed' bool dropped)
    if v.get('history_reset'): t['history_reset'] = True
    if v.get('refreshed_instances'): t['refreshed'] = v['refreshed_instances']
    # preview opened fine -> nothing; failure -> preview_error string
    rec(label, '3_trimmed', t)

# ---------- preview status ----------
v = real['preview_status']
rec('preview_status', '0_current', lines[names.index('preview_status')])
rec('preview_status', '1_f32', tidy(v))
t = {'open': v['open'], 'name': v['name'], 'revision': v['revision'], 'parts': v['parts'], 'size_cm': size_cm(v['bounds'])}
for k, d in (('playing', True), ('speed', 1.0), ('time', 0.0), ('turntable', False), ('yaw', 0.0), ('scale', 1.0)):
    if v[k] != d: t[k] = tidy(v[k])
rec('preview_status', '3_trimmed', t)

# ---------- studio_status ----------
v = real['studio_status']
rec('studio_status', '0_current', lines[names.index('studio_status')])
t = {'mode': v['mode'], 'world_tick': v['world_tick'], 'feedback': 'headless',
     'asset': {'name': v['asset']['name'], 'revision': v['asset']['revision']}}
rec('studio_status', '3_trimmed', t)

print(f"{'reply':16} {'stage':22} {'chars':>6} {'tok':>5}")
for r in rows: print(f"{r[0]:16} {r[1]:22} {r[2]:6} {r[3]:5}")
json.dump(rows, open(W + '/proto/rows.json', 'w'))
