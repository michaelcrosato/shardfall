#!/usr/bin/env python3
"""Asset benchmark harness (docs/ASSET_SYSTEM_PLAN.md §10): does a fresh agent, given only the docs
it would normally get, make better assets for fewer tokens?

  bench.py setup                                   venv with the pinned tokenizer
  bench.py build --arm A [--rev a9ca003]           release pav for an arm (A: worktree at a9ca003)
  bench.py prepare --gate GA --arm A --tasks all [--run 1]
  bench.py runs --gate GA [--arm A] [--pending]    run folders, for launching author agents
  bench.py note RUN key=value ...                  record what the runner reported (model, tok_run)
  bench.py score [--gate GA] [--bin PATH]          metrics and acceptance -> results/runs.tsv
  bench.py collect [--gate GA]                     final assets as text -> results/assets/
  bench.py report [--gate GA] [--vs GA/A]          gate summary (and ratios to a baseline) -> summary.json
  bench.py calib [--bin PATH] | --report           judge calibration: 12 planted-defect pairs, both orders
  bench.py votes --gate G1 --arm T [--bin PATH]    before/after images of each edit run, for judges
  bench.py pairs --gate G2 --arm F --vs GA/A       each authoring run above or below the baseline's, for judges
  bench.py judgment IMAGE JUDGE '{"pref":1,...}'   record one judge's answer -> results/judgments.tsv
  bench.py selftest                                two scripted replays: equal tok_x, cap enforced

Runs live in .cache/asset_bench/runs/<gate>/<arm>/<task>-r<k>/ (git-ignored). Each author run is
one fresh `asset-bench-author` agent (.claude/agents/) that reads RUN/brief.md and RUN/docs/ and
drives the engine only through scripts/asset_bench/pav RUN. Each judgment is one fresh
`asset-bench-judge` agent shown one image (.cache/asset_bench/judge/) and its brief.
"""

import argparse
import colorsys
import hashlib
import json
import math
import os
import random
import re
import shutil
import statistics
import struct
import subprocess
import sys
import time
import tomllib
import zlib
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BENCH = REPO / "scripts/asset_bench"
CACHE = REPO / ".cache/asset_bench"
RUNS, BIN, VENV, JUDGE = CACHE / "runs", CACHE / "bin", CACHE / "venv", CACHE / "judge"
IMAGES = JUDGE / "img"  # every judged image, under a name that says nothing about it
RESULTS = BENCH / "results"
JUDGMENTS = RESULTS / "judgments.tsv"
JUDGMENT_COLUMNS = ["kind", "image", "target", "order", "judge", "pref", "done", "s1", "s2", "why", "model",
                    "effort", "judged"]
# What judges see: the sheet every gate renders with the newest build (§10.1 Quality).
SHEET = {"views": True, "tile": 320, "elevation": 20, "figure": True}
# Calibration (§10.1): three templates and three Gate-A assets, each with two planted defects, so
# each of the six defects appears twice.
CALIB = [("crate", "colours"), ("crate", "spheres"), ("bench", "float"), ("bench", "delete"),
         ("lantern", "stretch"), ("lantern", "poke"), ("P01", "float"), ("P01", "colours"),
         ("P07", "poke"), ("P07", "spheres"), ("P19", "delete"), ("P19", "stretch")]
TIKTOKEN = "tiktoken==0.12.0"  # the newest release at least 12 months old (doctrine 9)
BASELINE_REV = "a9ca003"
S8 = ["P01", "P03", "P05", "P07", "P13", "P19", "P21", "P24"]
S8M = ["P03", "P05", "P06", "P11", "P17", "P19", "P22", "P24"]
DEFAULT_CAP = [30, 6]
# Reply fields that vary from run to run; token counts read them as 0 so tok_x is deterministic.
VOLATILE = re.compile(r'"([a-z_]*_ms)":\s*-?[0-9.eE+-]+')

README = """# Benchmark run

You are building game assets for a chunky, stylised, low-poly, cel-shaded game seen from a high
camera. `ASSET_STUDIO.md` in this folder documents the object tools. Ignore its parts about
starting the native studio, MCP setup and the human feedback loop: here every tool runs headless
through the run's wrapper, and RUN is the run folder named in your task:

    scripts/asset_bench/pav RUN <<'EOF'
    {{"tool":"asset_edit","args":{{"action":"inspect","name":"BUILTIN/bench"}}}}
    EOF

Each line is one command: a JSON line as above, or `tool key=value ...`. Every wrapper call starts
a fresh engine session. Saved assets persist; the preview stage and camera do not, so open the
preview and capture it in the same call, for example:

    asset_preview name=WORKSHOP/my_asset
    capture width=480 height=360 out=out/front.png

Images are written under RUN/out; look at each one with your image-reading tool. `help` lists every
tool. Budget: {commands} engine commands and {images} images; the wrapper refuses commands past it.
"""


# --- helpers -------------------------------------------------------------------------------

def tasks():
    with open(BENCH / "tasks.toml", "rb") as f:
        return {t["id"]: t for t in tomllib.load(f)["task"]}


def names(task):
    return task.get("names") or [task["name"]]


def select(spec, table):
    if spec in ("all", "authoring"):
        ids = [i for i, t in table.items() if t["kind"] == "author"]
        return ids + ([i for i, t in table.items() if t["kind"] == "edit"] if spec == "all" else [])
    if spec == "S8":
        return S8
    if spec == "S8m":
        return S8M
    if spec == "edits":
        return [i for i, t in table.items() if t["kind"] == "edit"]
    ids = [s.strip() for s in spec.split(",") if s.strip()]
    unknown = [i for i in ids if i not in table]
    if unknown:
        sys.exit(f"unknown task ids: {', '.join(unknown)}")
    return ids


def run_dirs(gate=None, arm=None):
    pattern = f"{gate or '*'}/{arm or '*'}/*"
    return sorted(p for p in RUNS.glob(pattern) if (p / "run.json").exists())


def load(path, default=None):
    try:
        with open(path) as f:
            return json.load(f)
    except (OSError, ValueError):
        return default


def git(*args):
    return subprocess.run(["git", *args], cwd=REPO, check=True, capture_output=True, text=True).stdout


def ensure_tokenizer():
    """Re-run this script under the venv when tiktoken is missing here."""
    try:
        import tiktoken  # noqa: F401
        return
    except ImportError:
        pass
    python = VENV / "bin/python"
    if not python.exists():
        sys.exit("run `scripts/asset_bench/bench.py setup` first (it installs the pinned tokenizer)")
    os.environ.setdefault("TIKTOKEN_CACHE_DIR", str(CACHE / "tiktoken"))
    os.execv(str(python), [str(python), "-I", str(Path(__file__).resolve()), *sys.argv[1:]])


def encoder():
    os.environ.setdefault("TIKTOKEN_CACHE_DIR", str(CACHE / "tiktoken"))
    import tiktoken
    return tiktoken.get_encoding("o200k_base")


def image_tokens(size):
    return math.ceil(size[0] / 28) * math.ceil(size[1] / 28)


def engine(binary, run, lines):
    """Run REPL lines against a run's assets with any pav binary; one reply per line."""
    env = dict(os.environ, PAV_ASSETS="props", PAV_CREATURES="creatures")
    out = subprocess.run([str(binary), "repl"], cwd=run, env=env, input="\n".join(lines) + "\n",
                         capture_output=True, text=True, timeout=600)
    replies = []
    for line in out.stdout.splitlines():
        try:
            replies.append(json.loads(line))
        except ValueError:
            replies.append({"error": line})
    return replies


# --- images: 8-bit RGBA PNGs with the standard library ------------------------------------------

def png_read(path):
    """(width, rows) of an 8-bit RGBA PNG, each row a bytearray."""
    data = Path(path).read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        sys.exit(f"{path} is not a PNG")
    pos, idat, width, height = 8, bytearray(), 0, 0
    while pos < len(data):
        (length,), kind = struct.unpack(">I", data[pos:pos + 4]), data[pos + 4:pos + 8]
        body = data[pos + 8:pos + 8 + length]
        if kind == b"IHDR":
            width, height, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", body)
            if (depth, color, interlace) != (8, 6, 0):
                sys.exit(f"{path}: expected 8-bit RGBA without interlacing")
        elif kind == b"IDAT":
            idat += body
        pos += 12 + length
    raw, stride = zlib.decompress(bytes(idat)), width * 4
    rows, prev = [], bytearray(stride)
    for y in range(height):
        start = y * (stride + 1)
        kind, line = raw[start], bytearray(raw[start + 1:start + 1 + stride])
        for i in range(stride):
            a = line[i - 4] if i >= 4 else 0
            b, c = prev[i], prev[i - 4] if i >= 4 else 0
            if kind == 1:
                line[i] = (line[i] + a) & 255
            elif kind == 2:
                line[i] = (line[i] + b) & 255
            elif kind == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif kind == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(line)
        prev = line
    return width, rows


def png_write(path, width, rows):
    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))
    raw = b"".join(b"\x00" + bytes(row) for row in rows)
    header = struct.pack(">IIBBBBB", width, len(rows), 8, 6, 0, 0, 0)
    Path(path).write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw, 6))
                           + chunk(b"IEND", b""))


def stack(images, out, gaps):
    """Images one above another; gaps[i] white rows follow image i."""
    loaded = [png_read(path) for path in images]
    width = max(w for w, _ in loaded)
    rows = []
    for (w, image), gap in zip(loaded, list(gaps) + [0]):
        rows += [row + bytearray([255] * 4 * (width - w)) for row in image]
        rows += [bytearray([255] * 4 * width) for _ in range(gap)]
    png_write(out, width, rows)


def newest(binary):
    """The engine that renders and scores: --bin, else this checkout's release or debug build."""
    if binary:
        return Path(binary).resolve()
    for path in (REPO / "target/release/pav", REPO / "target/debug/pav"):
        if path.exists():
            return path
    sys.exit("build pav first (cargo build -p pav_tools --bin pav) or pass --bin")


def sheet(binary, folder, name, out):
    """Render WORKSHOP/<name> from folder/props as judges see it (SHEET), to out."""
    replies = engine(binary, folder, [json.dumps({"tool": "asset_preview", "args": {"name": f"WORKSHOP/{name}", **SHEET}})])
    reply = replies[0] if replies else {}
    if "path" not in reply:
        sys.exit(f"no sheet of {name} in {folder}: {reply}")
    shutil.move(Path(folder) / reply["path"], out)


def try_sheet(binary, folder, name, out):
    """sheet(), or False when the asset does not draw (it does not load, or draws nothing)."""
    replies = engine(binary, folder, [json.dumps({"tool": "asset_preview", "args": {"name": f"WORKSHOP/{name}", **SHEET}})])
    reply = replies[0] if replies else {}
    if "path" not in reply:
        return False
    shutil.move(Path(folder) / reply["path"], out)
    return True


def image_id(*key):
    """A name that tells a judge nothing: a hash of what the image shows."""
    return hashlib.sha256("/".join(map(str, key)).encode()).hexdigest()[:12]


# --- judges ------------------------------------------------------------------------------------

def read_judgments():
    if not JUDGMENTS.exists():
        return []
    header, *body = JUDGMENTS.read_text().splitlines()
    return [dict(zip(header.split("\t"), line.split("\t"))) for line in body]


def write_judgments(rows):
    RESULTS.mkdir(parents=True, exist_ok=True)
    with open(JUDGMENTS, "w") as f:
        f.write("\t".join(JUDGMENT_COLUMNS) + "\n")
        for r in rows:
            f.write("\t".join(str(r.get(c, "")).replace("\t", " ").replace("\n", " ") for c in JUDGMENT_COLUMNS) + "\n")


def judge_keys():
    return load(JUDGE / "keys.json", {})


def save_keys(keys):
    JUDGE.mkdir(parents=True, exist_ok=True)
    with open(JUDGE / "keys.json", "w") as f:
        json.dump(keys, f, indent=1, sort_keys=True)


def part_volume(box):
    lo, hi = box
    return max(hi[0] - lo[0], 0) * max(hi[1] - lo[1], 0) * max(hi[2] - lo[2], 0)


def plant(asset, bounds, defect, rng):
    """Plant one §10.1 calibration defect in a format-1 definition, using its part bounds."""
    parts = asset["parts"]
    by_volume = sorted(parts, key=lambda k: part_volume(bounds[k]), reverse=True)

    def holds(below, above):
        """below carries above: its top meets above's bottom, they overlap in plan, above is bigger."""
        (a0, a1), (b0, b1) = bounds[below], bounds[above]
        touching = abs(a1[1] - b0[1]) <= 0.02 or b0[1] < a1[1] <= b1[1]
        overlap = all(a0[i] < b1[i] and b0[i] < a1[i] for i in (0, 2))
        return touching and overlap and part_volume(bounds[above]) > part_volume(bounds[below])
    if defect == "float":  # the largest part that rests on another rises 10 cm, leaving a gap
        held = [k for k in by_volume if any(holds(other, k) for other in parts if other != k)]
        part = parts[(held or by_volume)[0]]
        x, y, z = part.get("pos", [0, 0, 0])
        part["pos"] = [x, y + 0.10, z]
    elif defect == "stretch":  # the whole object 1.8 times as tall
        for part in parts.values():
            x, y, z = part.get("pos", [0, 0, 0])
            part["pos"] = [x, y * 1.8, z]
            shape = part["shape"]
            if shape["type"] in ("box", "rounded_box"):
                shape["half"][1] *= 1.8
            elif shape["type"] in ("cylinder", "capsule"):
                shape["half_height"] *= 1.8
    elif defect == "colours":  # random saturated colours
        for part in parts.values():
            r, g, b = colorsys.hsv_to_rgb(rng.random(), 1.0, 1.0)
            part["color"] = "#%02x%02x%02x" % (round(r * 255), round(g * 255), round(b * 255))
    elif defect == "delete":  # the largest part removed
        del parts[by_volume[0]]
    elif defect == "poke":  # the tallest supporting part pushed up 30% of its height, through what it holds
        supports = [k for k in parts if any(holds(k, other) for other in parts if other != k)]
        if not supports:
            sys.exit(f"{asset['name']}: no part holds another, so no poke defect")
        name = max(supports, key=lambda k: bounds[k][1][1] - bounds[k][0][1])
        x, y, z = parts[name].get("pos", [0, 0, 0])
        parts[name]["pos"] = [x, y + 0.3 * (bounds[name][1][1] - bounds[name][0][1]), z]
    elif defect == "spheres":  # every part replaced by its bounding sphere
        for name, part in parts.items():
            lo, hi = bounds[name]
            part["pos"] = [(lo[i] + hi[i]) / 2 for i in range(3)]
            part["shape"] = {"type": "sphere", "radius": max(0.001, math.dist(lo, hi) / 2)}
            for key in ("yaw", "pitch", "roll"):
                part.pop(key, None)
    else:
        sys.exit(f"unknown defect {defect}")
    return asset


# --- commands ------------------------------------------------------------------------------

def cmd_setup(args):
    python = VENV / "bin/python"
    if not python.exists():
        subprocess.run([sys.executable, "-m", "venv", str(VENV)], check=True)
    subprocess.run([str(python), "-m", "pip", "install", "-q", TIKTOKEN], check=True)
    env = dict(os.environ, TIKTOKEN_CACHE_DIR=str(CACHE / "tiktoken"))
    probe = "import tiktoken; print(len(tiktoken.get_encoding('o200k_base').encode('hello world')))"
    count = subprocess.run([str(python), "-I", "-c", probe], env=env, check=True, capture_output=True, text=True)
    print(f"{TIKTOKEN} ready in {VENV} (o200k 'hello world' = {count.stdout.strip()} tokens)")


def cmd_build(args):
    BIN.mkdir(parents=True, exist_ok=True)
    rev = args.rev or (BASELINE_REV if args.arm == "A" else "HEAD")
    sha = git("rev-parse", rev).strip()
    target = BIN / f"pav-{args.arm}"
    info = load(BIN / f"pav-{args.arm}.json", {})
    if target.exists() and info.get("sha") == sha and not args.force:
        print(f"{target} is already built from {sha[:7]}")
        return
    if rev == "HEAD":
        if git("status", "--porcelain", "--", "crates", "Cargo.toml", "Cargo.lock").strip():
            sys.exit("HEAD has uncommitted engine changes; commit them or pass --rev")
        subprocess.run(["cargo", "build", "--release", "--locked", "-q", "-p", "pav_tools", "--bin", "pav"],
                       cwd=REPO, check=True)
        shutil.copy2(REPO / "target/release/pav", target)
    else:
        source, build_dir = CACHE / f"src-{args.arm}", CACHE / f"target-{args.arm}"
        if not source.exists():
            git("worktree", "add", "--detach", str(source), sha)
        try:
            env = dict(os.environ, CARGO_TARGET_DIR=str(build_dir))
            subprocess.run(["cargo", "build", "--release", "--locked", "-q", "-p", "pav_tools", "--bin", "pav"],
                           cwd=source, env=env, check=True)
            shutil.copy2(build_dir / "release/pav", target)
        finally:
            git("worktree", "remove", "--force", str(source))
            shutil.rmtree(build_dir, ignore_errors=True)
    with open(BIN / f"pav-{args.arm}.json", "w") as f:
        json.dump({"arm": args.arm, "rev": rev, "sha": sha, "built": time.strftime("%Y-%m-%d %H:%M")}, f)
    print(f"built {target} from {sha[:7]}")


def brief(task, cap):
    lines = [f"# Task {task['id']}", ""]
    targets = names(task)
    made = ", ".join(f"WORKSHOP/{n}" for n in targets)
    if task["kind"] == "edit":
        sources = load(Path(task["_run"]) / "sources.json", [])
        lines += [f"Start from {', '.join('WORKSHOP/' + s for s in sources)}: your own earlier asset"
                  f"{'s' if len(sources) > 1 else ''} (already saved in this run).",
                  f"Make {made} from {'them' if len(sources) > 1 else 'it'}: {task['brief']}", ""]
    else:
        lines += [f"Make {made}: {task['brief']}", ""]
    if "size" in task:
        w, h, d = task["size"]
        lines.append(f"Size: {w} x {h} x {d} cm (width x height x depth, along x, y, z), each axis within "
                     f"±{round(task['tol'] * 100)}%.")
    needs = task.get("needs", [])
    it = "Each one" if len(targets) > 1 else "It"
    if "hang" in needs:
        lines.append(f"{it} hangs: its origin is the hook, and everything hangs below it.")
    elif "wall" in needs:
        lines.append(f"{it} mounts on a wall: its origin is on the wall surface and it extends toward +z.")
    else:
        lines.append(f"{it} stands on the floor: its origin is on the floor under its centre, and its front faces +z.")
    if "light" in needs:
        lines.append("It must give off light into its surroundings.")
    if "sockets" in needs:
        lines.append(f"Where your tools support sockets, give it: {', '.join(task['sockets'])}.")
    done = (json.dumps({"assets": [f"WORKSHOP/{n}" for n in targets], "notes": "<=20 words"}) if len(targets) > 1
            else json.dumps({"asset": f"WORKSHOP/{targets[0]}", "notes": "<=20 words"}))
    lines += ["", f"Budget: {cap[0]} engine commands and {cap[1]} images.",
              f"When you are done, write RUN/done.json as {done} and reply \"done\".", ""]
    return "\n".join(lines)


def cmd_prepare(args):
    table = tasks()
    info = load(BIN / f"pav-{args.arm}.json")
    if not info:
        sys.exit(f"build arm {args.arm} first: bench.py build --arm {args.arm}")
    docs = git("show", f"{info['sha']}:docs/ASSET_STUDIO.md")
    made = []
    for task_id in select(args.tasks, table):
        task = dict(table[task_id])
        if task["kind"] == "import":
            sys.exit(f"{task_id} is an import task; import arms are prepared from gate G5 on")
        run = RUNS / args.gate / args.arm / f"{task_id}-r{args.run}"
        if run.exists():
            if not args.force:
                print(f"skip {run.relative_to(REPO)}: exists")
                continue
            shutil.rmtree(run)
        (run / "docs").mkdir(parents=True)
        (run / "props/workshop").mkdir(parents=True)
        (run / "out").mkdir()
        cap = task.get("cap", DEFAULT_CAP)
        if task["kind"] == "edit":
            source_task = table[task["from"]]
            source_run = RUNS / args.gate / args.arm / f"{task['from']}-r{args.source_run}"
            sources = []
            for name in names(source_task):
                path = source_run / "props/workshop" / f"{name}.json"
                if path.exists():
                    shutil.copy2(path, run / "props/workshop" / path.name)
                    sources.append(name)
            if not sources:
                print(f"warning: {task_id} has no source asset from {source_run.relative_to(REPO)}")
            with open(run / "sources.json", "w") as f:
                json.dump(sources, f)
        task["_run"] = str(run)
        (run / "brief.md").write_text(brief(task, cap))
        (run / "docs/ASSET_STUDIO.md").write_text(docs)
        (run / "docs/README.md").write_text(README.format(commands=cap[0], images=cap[1]))
        config = {"gate": args.gate, "arm": args.arm, "task": task_id, "run": args.run, "kind": task["kind"],
                  "names": names(task), "bin": str(BIN / f"pav-{args.arm}"), "sha": info["sha"], "cap": cap,
                  "model": args.model, "effort": args.effort, "prepared": time.strftime("%Y-%m-%d %H:%M")}
        with open(run / "run.json", "w") as f:
            json.dump(config, f, indent=1)
        made.append(run)
    for run in made:
        print(run.relative_to(REPO))


def cmd_runs(args):
    for run in run_dirs(args.gate, args.arm):
        done = (run / "done.json").exists()
        if args.pending and done:
            continue
        state = load(run / "state.json", {})
        print(f"{run.relative_to(REPO)}\t{'done' if done else 'pending'}\t{state.get('commands', 0)} commands")


def cmd_note(args):
    run = Path(args.run).resolve()
    config = load(run / "run.json")
    if config is None:
        sys.exit(f"{run} is not a run")
    for pair in args.pairs:
        key, _, value = pair.partition("=")
        config.setdefault("runner", {})[key] = int(value) if value.isdigit() else value
    with open(run / "run.json", "w") as f:
        json.dump(config, f, indent=1)


def traffic(run, enc):
    """Per log row: (invocation, tokens of the command and reply, image tokens)."""
    rows = []
    prefix = str(run)
    for line in (run / "log.jsonl").read_text().splitlines() if (run / "log.jsonl").exists() else []:
        row = json.loads(line)
        reply = VOLATILE.sub(lambda m: f'"{m.group(1)}":0', row["reply"].replace(prefix, "RUN"))
        tokens = len(enc.encode(row["line"])) + len(enc.encode(reply))
        rows.append((row["inv"], tokens, image_tokens(row["img"]) if row.get("img") else 0, row))
    return rows


def token_metrics(run, enc):
    docs = sum(len(enc.encode(p.read_text())) for p in sorted((run / "docs").glob("*")))
    rows = traffic(run, enc)
    per_turn = {}
    for inv, tokens, image, _ in rows:
        per_turn[inv] = per_turn.get(inv, 0) + tokens + image
    context, cumulative = docs, 0
    for inv in sorted(per_turn):
        cumulative += context  # the agent reads its context, then makes this call
        context += per_turn[inv]
    cumulative += context  # the final turn reads everything and reports
    return {"tok_x": context, "tok_cum": cumulative, "turns": len(per_turn), "doc_tok": docs}, rows


def first_edit_ok(rows):
    for _, _, _, row in rows:
        line = row["line"]
        if row["refused"]:
            continue
        try:
            value = json.loads(line) if line.strip().startswith("{") else None
        except ValueError:
            value = None
        tool = (value or {}).get("tool") if value else line.split()[0] if line.split() else ""
        args = (value or {}).get("args", {}) if value else dict(w.split("=", 1) for w in line.split()[1:] if "=" in w)
        if tool == "asset_edit" and args.get("action") in ("create", "copy", "replace"):
            return not row["reply"].startswith('{"error"'), args.get("from", "")
    return None, ""


def check_asset(binary, run, name, needs, size, tol):
    """Acceptance facts for one saved asset, read with the newest engine."""
    replies = engine(binary, run, [json.dumps({"tool": "asset_edit", "args": {
        "action": "inspect", "name": f"WORKSHOP/{name}", "bounds": True}}),
        json.dumps({"tool": "asset_edit", "args": {"action": "check", "name": f"WORKSHOP/{name}", "all": True}})])
    inspect = replies[0] if replies else {"error": "no reply"}
    facts = {"loads": bool(inspect.get("asset")), "problems": []}
    if not facts["loads"]:
        facts["problems"].append(f"{name}: does not load ({str(inspect.get('error', 'absent'))[:120]})")
        return facts
    lo, hi = inspect["bounds"]
    cm = [round((hi[i] - lo[i]) * 100, 1) for i in range(3)]
    # Drawn parts after copies and mirrors, where the engine lists them (format 1: its parts).
    parts = inspect.get("part_bounds") or inspect["asset"]["parts"]
    facts.update(parts=len(parts), size_cm="x".join(f"{c:g}" for c in cm))
    if size:
        for axis, actual, target in zip("xyz", cm, size):
            if abs(actual - target) > tol * target:
                facts["problems"].append(f"{name}: {axis} {actual:g} cm, target {target} ±{round(tol * 100)}%")
    min_y, max_y, min_z = lo[1] * 100, hi[1] * 100, lo[2] * 100
    if "hang" in needs:
        if abs(max_y) > 2:
            facts["problems"].append(f"{name}: hangs from {max_y:.1f} cm, not 0")
    elif "wall" in needs:
        if not -1 <= min_z <= 2:
            facts["problems"].append(f"{name}: back at z {min_z:.1f} cm, not on the wall")
    elif min_y < -0.5:
        facts["problems"].append(f"{name}: below_floor ({min_y:.1f} cm)")
    elif min_y > 0.5:
        facts["problems"].append(f"{name}: not_grounded ({min_y:.1f} cm)")
    check = replies[1] if len(replies) > 1 else {}
    # Light: a light field, or a part glowing above emissive 0.5 (the cost line counts both, in
    # either format, materials and groups included).
    lights = re.search(r"(\d+) lights", check.get("cost", ""))
    if lights is None:
        parts = inspect["asset"]["parts"].values()
        glows = any(p.get("emissive", 0) > 0.5 or p.get("light") for p in parts)
    else:
        glows = int(lights.group(1)) > 0
    if "light" in needs and not glows:
        facts["problems"].append(f"{name}: gives off no light")
    if "error" in check:
        facts["check"] = "na"  # the scoring engine has no lint; score with a newer one
    else:
        facts.update(check="ok", lint_block=0, lint_other=0)
        blocking, expected = {"floating", "unit_suspect"}, set()
        if "hang" in needs or "wall" in needs:
            expected = {"below_floor", "not_grounded"}  # off the floor by design
        else:
            blocking |= {"below_floor", "not_grounded"}
        for line in check.get("check", []):
            code = line.split(":")[0]
            if code in blocking:
                facts["problems"].append(f"{name}: {line}")
                facts["lint_block"] += 1
            elif code not in expected:
                facts["lint_other"] += 1
        cost = check.get("cost", "")
        facts["tris"] = int(cost.split()[0]) if cost[:1].isdigit() else 0
    return facts


def cmd_score(args):
    ensure_tokenizer()
    enc = encoder()
    binary = Path(args.bin).resolve() if args.bin else (BIN / "pav-HEAD" if (BIN / "pav-HEAD").exists()
                                                         else REPO / "target/debug/pav")
    table = tasks()
    judgments = read_judgments()
    columns = ["gate", "arm", "task", "run", "done", "accepted", "tok_x", "tok_cum", "tok_run", "turns", "calls",
               "failed", "refused", "first_ok", "lint_block", "lint_other", "check", "wall_s", "engine_ms", "parts",
               "tris", "size_cm", "copied_from", "model", "effort", "problems"]
    rows = []
    for run in run_dirs(args.gate):
        config = load(run / "run.json")
        task = table[config["task"]]
        metrics, log = token_metrics(run, enc)
        done = load(run / "done.json") is not None
        first_ok, copied = first_edit_ok(log)
        problems, parts, sizes, check, lint, tris = [], 0, [], "na", [0, 0], 0
        for name in config["names"]:
            facts = check_asset(binary, run, name, task.get("needs", []), task.get("size"), task.get("tol", 0.15))
            problems += facts["problems"]
            parts += facts.get("parts", 0)
            sizes.append(facts.get("size_cm", "-"))
            check = facts.get("check", check)
            lint = [lint[0] + facts.get("lint_block", 0), lint[1] + facts.get("lint_other", 0)]
            tris += facts.get("tris", 0)
        if task["kind"] == "edit":
            # Edits also need the change made: at least 2 of 3 judges on a before/after image.
            target = f"{config['gate']}/{config['arm']}/{config['task']}-r{config['run']}"
            votes = [j for j in judgments if j["kind"] == "edit" and j["target"] == target]
            made = sum(1 for j in votes if j["done"] == "1")
            if len(votes) < 3:
                problems.append(f"edit: {len(votes)} of 3 judge votes so far")
            elif made < 2:
                problems.append(f"edit: {made} of {len(votes)} judges saw the change made")
        logged = [r for _, _, _, r in log]
        times = [r["t"] for r in logged]
        wall = round(max(r["t"] + r["ms"] / 1000 for r in logged) - min(times), 1) if logged else 0
        runner = config.get("runner", {})
        rows.append({
            "gate": config["gate"], "arm": config["arm"], "task": config["task"], "run": config["run"],
            "done": int(done), "accepted": int(done and not problems), **metrics,
            "tok_run": runner.get("tok_run", ""), "calls": sum(1 for r in logged if not r["refused"]),
            "failed": sum(1 for r in logged if not r["refused"] and r["reply"].startswith('{"error"')),
            "refused": sum(1 for r in logged if r["refused"]),
            "first_ok": "" if first_ok is None else int(first_ok), "lint_block": "na" if check == "na" else lint[0],
            "lint_other": "na" if check == "na" else lint[1], "check": check, "wall_s": wall,
            "engine_ms": sum(r["ms"] for r in logged), "parts": parts, "tris": "na" if check == "na" else tris,
            "size_cm": "/".join(sizes),
            "copied_from": copied, "model": runner.get("model", config.get("model", "")),
            "effort": runner.get("effort", config.get("effort", "")), "problems": "; ".join(problems)})
    RESULTS.mkdir(parents=True, exist_ok=True)
    existing = []
    path = RESULTS / "runs.tsv"
    if path.exists() and args.gate:
        # Keep other gates' rows; rescoring one gate replaces only its own.
        header, *body = path.read_text().splitlines()
        existing = [dict(zip(header.split("\t"), line.split("\t"))) for line in body]
        existing = [r for r in existing if r["gate"] != args.gate]
    all_rows = existing + [{k: str(v) for k, v in r.items()} for r in rows]
    all_rows.sort(key=lambda r: (r["gate"], r["arm"], r["task"], int(r["run"])))
    with open(path, "w") as f:
        f.write("\t".join(columns) + "\n")
        for r in all_rows:
            f.write("\t".join(str(r.get(c, "")).replace("\t", " ") for c in columns) + "\n")
    print(f"scored {len(rows)} runs with {binary} -> {path.relative_to(REPO)}")


def cmd_collect(args):
    count = 0
    for run in run_dirs(args.gate):
        config = load(run / "run.json")
        dest = RESULTS / "assets" / config["gate"] / config["arm"]
        dest.mkdir(parents=True, exist_ok=True)
        for name in config["names"]:
            source = run / "props/workshop" / f"{name}.json"
            if source.exists():
                suffix = f"-{name}" if len(config["names"]) > 1 else ""
                shutil.copy2(source, dest / f"{config['task']}-r{config['run']}{suffix}.json")
                count += 1
    print(f"collected {count} assets into {(RESULTS / 'assets').relative_to(REPO)}")


def read_runs():
    path = RESULTS / "runs.tsv"
    if not path.exists():
        sys.exit("score first: bench.py score")
    header, *body = path.read_text().splitlines()
    return [dict(zip(header.split("\t"), line.split("\t"))) for line in body]


def compare(mine, baseline):
    """§10.2 ratios: per task, this arm's tokens over the baseline's mean on that task (median over
    tasks); acceptance and lint_other per accepted asset against the baseline's run 1 of the same tasks."""
    gate, arm = baseline.split("/")
    base = [r for r in read_runs() if r["gate"] == gate and r["arm"] == arm]
    ratios = {"tok_x": [], "tok_cum": []}
    for r in mine:
        theirs = [b for b in base if b["task"] == r["task"]]
        for metric in ratios:
            if theirs:
                ratios[metric].append(int(r[metric]) / statistics.mean(int(b[metric]) for b in theirs))
    matched = [b for b in base if b["run"] == "1" and b["task"] in {r["task"] for r in mine}]

    def lint_per_accepted(rows):
        accepted = [r for r in rows if r["accepted"] == "1" and r["lint_other"] not in ("", "na")]
        return round(sum(int(r["lint_other"]) for r in accepted) / len(accepted), 3) if accepted else None
    return {
        "tasks": len(ratios["tok_x"]),
        "median_tok_x_ratio": round(statistics.median(ratios["tok_x"]), 3) if ratios["tok_x"] else None,
        "median_tok_cum_ratio": round(statistics.median(ratios["tok_cum"]), 3) if ratios["tok_cum"] else None,
        "accepted": sum(int(r["accepted"]) for r in mine), "baseline_accepted": sum(int(b["accepted"]) for b in matched),
        "lint_other_per_accepted": lint_per_accepted(mine), "baseline_lint_other_per_accepted": lint_per_accepted(matched),
    }


# §10.2 thresholds per gate: token ratios are medians over tasks, against the baseline's mean.
GATES = {"G2": {"tasks": "all", "authoring_tok_x": 0.75, "authoring_tok_cum": 0.65, "edits_tok_x": 0.60, "pairs": 24,
                "parity": 12, "rubric": -0.1, "k1_tok_x": 0.90, "k1_wins": 15, "k2": 8, "k3_points": 15,
                "k3_failed": 3}}


def gate_verdict(gate, key, mine, vs):
    """The §10.2 pass checks and kill criteria of a gate with authoring, edit and pair results."""
    limit = GATES[gate]
    expected = set(select(limit["tasks"], tasks()))
    present = {r["task"] for r in mine}
    authoring = compare([r for r in mine if r["task"].startswith("P")], vs)
    edits = compare([r for r in mine if r["task"].startswith("E")], vs)
    pairs = pair_results(key + "/")
    judged = [p for p in pairs.values() if p["complete"]]
    outcomes = [p["outcome"] for p in judged]
    wins, ties, losses = outcomes.count("win"), outcomes.count("tie"), outcomes.count("loss")
    deltas = [p["delta"] for p in judged if p["delta"] is not None]
    rubric_delta = round(statistics.mean(deltas), 3) if deltas else None
    whole = compare(mine, vs)
    accepted, theirs, runs = whole["accepted"], whole["baseline_accepted"], len(mine)
    failed = statistics.median(int(r["failed"]) for r in mine) if mine else None
    points = round((accepted - theirs) / runs * 100, 1) if runs else 0
    score = wins + ties / 2
    # A verdict needs every planned run and every pair answered by three judges.
    all_runs, all_pairs = expected <= present, len(judged) == limit["pairs"]
    checks = {
        "runs": (f"{len(expected & present)}/{len(expected)}", all_runs),
        "authoring_tok_x": (authoring["median_tok_x_ratio"], authoring["median_tok_x_ratio"] <= limit["authoring_tok_x"]),
        "authoring_tok_cum": (authoring["median_tok_cum_ratio"],
                              authoring["median_tok_cum_ratio"] <= limit["authoring_tok_cum"]),
        "edits_tok_x": (edits["median_tok_x_ratio"], edits["median_tok_x_ratio"] <= limit["edits_tok_x"]),
        "acceptance": (f"{accepted}/{runs} vs {theirs}", accepted >= theirs - 1),
        "median_failed": (failed, failed is not None and failed <= 1),
        "wins_ties": (f"{wins}+{ties}/2 of {len(judged)} judged", all_pairs and score >= limit["parity"]),
        "rubric_delta": (rubric_delta, all_pairs and rubric_delta is not None and rubric_delta >= limit["rubric"]),
    }
    kills = {
        "K1": all_pairs and authoring["median_tok_x_ratio"] > limit["k1_tok_x"] and wins < limit["k1_wins"],
        "K2": all_pairs and score <= limit["k2"],
        "K3": all_runs and (points < -limit["k3_points"] or failed >= limit["k3_failed"]),
    }
    return {"checks": {k: {"value": v, "pass": bool(ok)} for k, (v, ok) in checks.items()},
            "pairs": {"wins": wins, "ties": ties, "losses": losses, "judged": len(judged)},
            "acceptance_points": points, "kills": {k: bool(v) for k, v in kills.items()},
            "passed": all(ok for _, ok in checks.values()), "killed": any(kills.values())}


def cmd_report(args):
    rows = [r for r in read_runs() if not args.gate or r["gate"] == args.gate]
    summary = load(RESULTS / "summary.json", {})
    for gate in sorted({r["gate"] for r in rows}):
        for arm in sorted({r["arm"] for r in rows if r["gate"] == gate}):
            mine = [r for r in rows if r["gate"] == gate and r["arm"] == arm]
            done = sum(int(r["done"]) for r in mine)
            authoring = [r for r in mine if r["task"].startswith("P")]
            pairs = {}
            for r in authoring:
                pairs.setdefault(r["task"], {})[int(r["run"])] = int(r["tok_x"])
            spread = [abs(math.log(p[1] / p[2])) for p in pairs.values() if 1 in p and 2 in p and p[1] and p[2]]
            key = f"{gate}/{arm}"
            summary[key] = {
                "runs": len(mine), "done": done, "done_rate": round(done / len(mine), 3) if mine else 0,
                "accepted": sum(int(r["accepted"]) for r in mine),
                "check": sorted({r["check"] for r in mine}),
                "median_tok_x": statistics.median(int(r["tok_x"]) for r in mine) if mine else 0,
                "median_tok_cum": statistics.median(int(r["tok_cum"]) for r in mine) if mine else 0,
                "median_failed": statistics.median(int(r["failed"]) for r in mine) if mine else 0,
                "first_ok": sum(1 for r in mine if r["first_ok"] == "1"),
                "s8_pairs": len(spread),
                "median_abs_ln_tok_x_ratio": round(statistics.median(spread), 4) if spread else None,
                "models": sorted({r["model"] for r in mine}),
            }
            s = summary[key]
            print(f"{key}: {s['done']}/{s['runs']} done ({s['done_rate']:.0%}), {s['accepted']} accepted, "
                  f"median tok_x {s['median_tok_x']:g}, tok_cum {s['median_tok_cum']:g}, "
                  f"failed {s['median_failed']:g}; S8 |ln ratio| median {s['median_abs_ln_tok_x_ratio']} "
                  f"over {s['s8_pairs']} pairs (ln 1.5 = {math.log(1.5):.3f})")
            if args.vs and args.vs != key:
                s["vs"] = compare(mine, args.vs)
                print(f"  vs {args.vs}: {json.dumps(s['vs'])}")
                if gate in GATES:
                    s["gate"] = gate_verdict(gate, key, mine, args.vs)
                    print(f"  {gate}: {json.dumps(s['gate'])}")
    with open(RESULTS / "summary.json", "w") as f:
        json.dump(summary, f, indent=1, sort_keys=True)
        f.write("\n")


def calib_sources(table):
    """(name, definition, brief) of each calibration source asset."""
    sources = {}
    for source, _ in CALIB:
        if source in sources:
            continue
        if source in table:
            path = RESULTS / "assets/GA/A" / f"{source}-r1.json"
            task = table[source]
            w, h, d = task["size"]
            brief = f"{task['brief']} About {w} x {h} x {d} cm (width x height x depth)."
        else:
            path = REPO / "assets/props/templates" / f"{source}.json"
            brief = None
        asset = json.loads(path.read_text())
        sources[source] = (asset["name"], asset, brief or asset["description"])
    return sources


def cmd_calib(args):
    if args.report:
        return calib_report()
    binary = newest(args.bin)
    table = tasks()
    folder = JUDGE / "work/calib"
    shutil.rmtree(folder, ignore_errors=True)
    (folder / "props/workshop").mkdir(parents=True)
    (folder / "sheets").mkdir()
    IMAGES.mkdir(parents=True, exist_ok=True)
    keys = judge_keys()
    for name in [k for k, v in keys.items() if v["kind"] == "calib"]:
        Path(keys.pop(name)["path"]).unlink(missing_ok=True)
    rng = random.Random(1)
    for number, (source, defect) in enumerate(CALIB, 1):
        _, asset, brief = calib_sources(table)[source]
        pair = f"c{number:02d}"
        intact = dict(asset, name=f"{pair}_a")
        (folder / "props/workshop" / f"{pair}_a.json").write_text(json.dumps(intact))
        bounds = engine(binary, folder, [json.dumps({"tool": "asset_edit", "args": {
            "action": "inspect", "name": f"WORKSHOP/{pair}_a", "bounds": True}})])[0]["part_bounds"]
        broken = plant(json.loads(json.dumps(intact)) | {"name": f"{pair}_b"}, bounds, defect, rng)
        (folder / "props/workshop" / f"{pair}_b.json").write_text(json.dumps(broken))
        for side in "ab":
            sheet(binary, folder, f"{pair}_{side}", folder / "sheets" / f"{pair}_{side}.png")
        for order, top, bottom, intact_at in (("intact-top", "a", "b", 1), ("intact-bottom", "b", "a", 2)):
            name = image_id("calib", pair, order, binary.stat().st_mtime_ns)
            stack([folder / "sheets" / f"{pair}_{top}.png", folder / "sheets" / f"{pair}_{bottom}.png"],
                  IMAGES / f"{name}.png", [8])
            keys[name] = {"kind": "calib", "target": pair, "order": order, "intact": intact_at, "source": source,
                          "defect": defect, "brief": brief, "path": str(IMAGES / f"{name}.png")}
    save_keys(keys)
    for name, key in sorted(keys.items(), key=lambda kv: kv[1]["path"]):
        if key["kind"] == "calib":
            print(f"{name}\t{key['path']}\t{key['brief']}")


def majority(rows, field):
    """The value most judges gave, or None on a tie."""
    counts = {}
    for r in rows:
        counts[r[field]] = counts.get(r[field], 0) + 1
    best = max(counts.values(), default=0)
    winners = [v for v, n in counts.items() if n == best]
    return winners[0] if len(winners) == 1 and best * 2 > len(rows) else None


def calib_report():
    rows = [r for r in read_judgments() if r["kind"] == "calib"]
    pairs = sorted({r["target"] for r in rows})
    picks, intact_wins, flips = {}, {"intact-top": 0, "intact-bottom": 0}, 0
    for pair in pairs:
        for order, intact_at in (("intact-top", "1"), ("intact-bottom", "2")):
            votes = [r for r in rows if r["target"] == pair and r["order"] == order]
            pick = majority(votes, "pref")
            picks[(pair, order)] = None if pick is None else pick == intact_at
            intact_wins[order] += int(pick == intact_at)
        flips += int(picks[(pair, "intact-top")] != picks[(pair, "intact-bottom")])
    complete = all(sum(1 for r in rows if r["target"] == p and r["order"] == o) >= 3
                   for p in pairs for o in intact_wins)
    passed = complete and len(pairs) == len(CALIB) and min(intact_wins.values()) >= 10 and flips <= 2
    result = {"pairs": len(pairs), "judgments": len(rows), "intact_picked": intact_wins, "order_flips": flips,
              "missed": sorted(f"{p} {o}" for (p, o), ok in picks.items() if not ok), "passed": passed}
    summary = load(RESULTS / "summary.json", {})
    summary["calibration"] = result
    with open(RESULTS / "summary.json", "w") as f:
        json.dump(summary, f, indent=1, sort_keys=True)
        f.write("\n")
    print(json.dumps(result))


def cmd_votes(args):
    """Before (the source run's asset) above after (the edit run's asset), one image per edit run."""
    binary = newest(args.bin)
    table = tasks()
    keys = judge_keys()
    folder = JUDGE / "work" / f"edit-{args.gate}-{args.arm}"
    shutil.rmtree(folder, ignore_errors=True)
    (folder / "props/workshop").mkdir(parents=True)
    (folder / "sheets").mkdir()
    IMAGES.mkdir(parents=True, exist_ok=True)
    for run in run_dirs(args.gate, args.arm):
        config = load(run / "run.json")
        task = table[config["task"]]
        if task["kind"] != "edit":
            continue
        target = f"{args.gate}/{args.arm}/{config['task']}-r{config['run']}"
        source_run = RUNS / args.gate / args.arm / f"{task['from']}-r1"
        befores, afters = [], []
        for before, after in zip(names(table[task["from"]]), config["names"]):
            for tag, path, out in (("before", source_run / "props/workshop" / f"{before}.json", befores),
                                   ("after", run / "props/workshop" / f"{after}.json", afters)):
                leaf = f"{config['task'].lower()}_{tag}_{len(out)}"
                if path.exists():
                    asset = json.loads(path.read_text()) | {"name": leaf}
                    (folder / "props/workshop" / f"{leaf}.json").write_text(json.dumps(asset))
                    sheet(binary, folder, leaf, folder / "sheets" / f"{leaf}.png")
                    out.append(folder / "sheets" / f"{leaf}.png")
        if not afters:
            print(f"skip {target}: no edited asset")
            continue
        name = image_id("edit", target, binary.stat().st_mtime_ns)
        gaps = [8] * (len(befores) - 1) + [40] + [8] * (len(afters) - 1)
        stack(befores + afters, IMAGES / f"{name}.png", gaps)
        keys[name] = {"kind": "edit", "target": target, "order": "before-top", "brief": task["brief"],
                      "path": str(IMAGES / f"{name}.png")}
        print(f"{name}\t{IMAGES / f'{name}.png'}\t{task['brief']}")
    save_keys(keys)


def task_brief(task):
    """What a judge reads with a comparison: the brief and, when it has one, the size."""
    brief = task["brief"]
    if "size" in task:
        w, h, d = task["size"]
        brief += f" About {w} x {h} x {d} cm (width x height x depth)."
    return brief


def cmd_pairs(args):
    """Each authoring run of an arm above or below the baseline's run of the same task (§10.1
    Quality), in an order drawn from the run's name, for three judges. A task with several
    assets stacks each side's sheets. A side with nothing to draw loses without judges."""
    binary = newest(args.bin)
    table = tasks()
    base_gate, base_arm = args.vs.split("/")
    keys = judge_keys()
    prefix = f"{args.gate}/{args.arm}/"
    for name in [k for k, v in keys.items() if v["kind"] == "pair" and v["target"].startswith(prefix)]:
        Path(keys.pop(name)["path"]).unlink(missing_ok=True)
    folder = JUDGE / "work" / f"pairs-{args.gate}-{args.arm}"
    shutil.rmtree(folder, ignore_errors=True)
    (folder / "props/workshop").mkdir(parents=True)
    (folder / "sheets").mkdir()
    IMAGES.mkdir(parents=True, exist_ok=True)
    unjudged = []
    for run in run_dirs(args.gate, args.arm):
        config = load(run / "run.json")
        task = table[config["task"]]
        if task["kind"] != "author":
            continue
        target = f"{prefix}{config['task']}-r{config['run']}"
        sides = {}
        for side, source in (("arm", run), ("base", RUNS / base_gate / base_arm / f"{config['task']}-r{config['run']}")):
            sheets = []
            for i, asset_name in enumerate(names(task)):
                path = source / "props/workshop" / f"{asset_name}.json"
                leaf = f"{config['task'].lower()}_{side}_{i}"
                out = folder / "sheets" / f"{leaf}.png"
                if path.exists():
                    (folder / "props/workshop" / f"{leaf}.json").write_text(
                        json.dumps(json.loads(path.read_text()) | {"name": leaf}))
                    if try_sheet(binary, folder, leaf, out):
                        sheets.append(out)
            # A set is judged only whole.
            sides[side] = sheets if len(sheets) == len(names(task)) else []
        name = image_id("pair", target, args.vs, binary.stat().st_mtime_ns)
        key = {"kind": "pair", "target": target, "vs": args.vs, "brief": task_brief(task),
               "path": str(IMAGES / f"{name}.png")}
        if not sides["arm"] or not sides["base"]:
            missing = "both" if sides["arm"] == sides["base"] else "arm" if not sides["arm"] else "base"
            keys[name] = key | {"order": f"missing-{missing}"}
            unjudged.append({"kind": "pair", "image": name, "target": target, "order": f"missing-{missing}",
                             "judge": "-", "judged": time.strftime("%Y-%m-%d %H:%M")})
            print(f"{target}: nothing to draw on the {missing} side; no judges")
            continue
        arm_top = random.Random(target).random() < 0.5
        top, bottom = (sides["arm"], sides["base"]) if arm_top else (sides["base"], sides["arm"])
        count = len(names(task))
        stack(top + bottom, IMAGES / f"{name}.png", [8] * (count - 1) + [40 if count > 1 else 8] + [8] * (count - 1))
        if count > 1:
            key["brief"] += (f" Each asset is a set of {count} objects: the top {count} rows are asset 1, the bottom"
                             f" {count} asset 2.")
        keys[name] = key | {"order": "arm-top" if arm_top else "arm-bottom", "arm_at": 1 if arm_top else 2}
        print(f"{name}\t{key['path']}\t{key['brief']}")
    save_keys(keys)
    if unjudged:
        stale = {r["target"] for r in unjudged}
        write_judgments([r for r in read_judgments() if not (r["kind"] == "pair" and r["target"] in stale
                                                            and r["order"].startswith("missing-"))] + unjudged)


def rubric(text):
    """A judgment's five 1-5 scores (its s1 or s2 column), or None when they are malformed."""
    try:
        scores = [int(x) for x in text.split(",")]
    except ValueError:
        return None
    return scores if len(scores) == 5 and all(1 <= x <= 5 for x in scores) else None


def pair_results(prefix):
    """Per pair: the majority's verdict for the arm and the rubric difference (the arm's median
    score minus the baseline's, averaged over the five criteria). Read from the committed
    judgments alone, so a fresh checkout reports the same; a pair counts once three distinct
    judges have answered its newest image, and a side with nothing to draw loses unjudged."""
    rows = [r for r in read_judgments() if r["kind"] == "pair" and r["target"].startswith(prefix)]
    newest = {}
    for r in sorted(rows, key=lambda r: (r["judged"], r["image"])):
        newest[r["target"]] = r["image"]  # a re-rendered pair is judged afresh
    results = {}
    for target, image in newest.items():
        mine = [r for r in rows if r["target"] == target and r["image"] == image]
        order = mine[0]["order"]
        if order.startswith("missing-"):
            outcome = {"arm": "loss", "base": "win", "both": "tie"}[order.removeprefix("missing-")]
            results[target] = {"outcome": outcome, "judges": 0, "delta": None, "complete": True}
            continue
        votes = {r["judge"]: r for r in mine if r["pref"] in ("0", "1", "2") and rubric(r["s1"]) and rubric(r["s2"])}
        if len(votes) < 3:
            results[target] = {"outcome": None, "judges": len(votes), "delta": None, "complete": False}
            continue
        pick, at = majority(list(votes.values()), "pref"), "1" if order == "arm-top" else "2"
        outcome = "win" if pick == at else "tie" if pick in ("0", None) else "loss"

        def medians(field):
            return [statistics.median(c) for c in zip(*(rubric(r[field]) for r in votes.values()))]
        mine_s, theirs = (medians("s1"), medians("s2")) if at == "1" else (medians("s2"), medians("s1"))
        delta = statistics.mean(a - b for a, b in zip(mine_s, theirs))
        results[target] = {"outcome": outcome, "judges": len(votes), "delta": delta, "complete": True}
    return results


def cmd_judgment(args):
    key = judge_keys().get(args.image)
    if key is None:
        sys.exit(f"unknown image {args.image} (bench.py calib / votes list them)")
    try:
        answer = json.loads(args.answer)
    except ValueError:
        sys.exit(f"the judge's answer is not JSON: {args.answer[:200]}")
    if key["kind"] == "edit":
        if not isinstance(answer.get("done"), bool):
            sys.exit(f"an edit vote needs done: true|false: {answer}")
    elif type(answer.get("pref")) is not int or answer["pref"] not in (0, 1, 2) or not all(
            isinstance(answer.get(k), list) and len(answer[k]) == 5
            and all(type(x) is int and 1 <= x <= 5 for x in answer[k]) for k in ("s1", "s2")):
        sys.exit(f"a comparison needs pref 0|1|2 and five whole scores 1-5 each in s1 and s2: {answer}")
    rows = [r for r in read_judgments() if not (r["image"] == args.image and r["judge"] == str(args.judge))]
    rows.append({"kind": key["kind"], "image": args.image, "target": key["target"], "order": key["order"],
                 "judge": args.judge, "pref": answer.get("pref", ""),
                 "done": "" if "done" not in answer else int(answer["done"]),
                 "s1": ",".join(map(str, answer.get("s1", []))), "s2": ",".join(map(str, answer.get("s2", []))),
                 "why": str(answer.get("why", ""))[:160], "model": args.model, "effort": args.effort,
                 "judged": time.strftime("%Y-%m-%d %H:%M")})
    rows.sort(key=lambda r: (r["kind"], r["target"], r["order"], r["image"], str(r["judge"])))
    write_judgments(rows)


def cmd_selftest(args):
    ensure_tokenizer()
    enc = encoder()
    arm = "A" if (BIN / "pav-A").exists() else None
    binary = BIN / "pav-A" if arm else REPO / "target/debug/pav"
    script = (BENCH / "selftest.cmds").read_text().splitlines()  # the planning dogfood stall session
    script = [line for line in script if line.strip()]
    script += ["assets"] * (31 - len(script))  # the 31st command must be refused
    results = []
    for k in (1, 2):
        run = CACHE / "selftest" / f"replay{k}"
        shutil.rmtree(run, ignore_errors=True)
        (run / "docs").mkdir(parents=True)
        (run / "out").mkdir()
        (run / "docs/README.md").write_text(README.format(commands=30, images=6))
        with open(run / "run.json", "w") as f:
            json.dump({"bin": str(binary), "cap": [30, 6], "names": ["market_stall"]}, f)
        # Two wrapper calls, as an agent makes them, so turns and tok_cum are exercised too.
        half = len(script) // 2
        for chunk in (script[:half], script[half:]):
            subprocess.run([str(BENCH / "pav"), str(run)], input="\n".join(chunk) + "\n", text=True,
                           capture_output=True, check=True)
        metrics, rows = token_metrics(run, enc)
        refused = [r for _, _, _, r in rows if r["refused"]]
        results.append((metrics, len(rows), len(refused)))
    (m1, n1, r1), (m2, n2, r2) = results
    ok = m1 == m2 and n1 == n2 == 31 and r1 == r2 == 1
    print(f"replay 1: {m1}, {n1} commands, {r1} refused\nreplay 2: {m2}, {n2} commands, {r2} refused")
    pairs_ok = pair_rules_hold()
    print("pair rules " + ("hold" if pairs_ok else "FAILED"))
    print("selftest " + ("passed" if ok and pairs_ok else "FAILED"))
    sys.exit(0 if ok and pairs_ok else 1)


def pair_rules_hold():
    """pair_results on made-up judgments: three distinct valid judges decide a pair, fewer (or a
    malformed score) leave it open, a newer image replaces an older one, a missing side loses."""
    global read_judgments
    vote = lambda image, target, order, judge, pref, s1="4,4,4,4,4", when="2026-01-01 10:00": {
        "kind": "pair", "image": image, "target": target, "order": order, "judge": str(judge),
        "pref": str(pref), "s1": s1, "s2": "3,3,3,3,3", "judged": when}
    rows = [vote("a", "T/X/P01-r1", "arm-top", j, 1) for j in (1, 2, 3)]
    rows += [vote("b", "T/X/P02-r1", "arm-bottom", j, 1) for j in (1, 2)]
    rows += [vote("c", "T/X/P03-r1", "arm-top", j, 1, "7,4,4,4,4" if j == 3 else "4,4,4,4,4") for j in (1, 2, 3)]
    rows += [vote("d", "T/X/P04-r1", "arm-top", j, 1) for j in (1, 2, 3)]
    rows += [vote("e", "T/X/P04-r1", "arm-bottom", j, 1, when="2026-01-02 10:00") for j in (1, 2, 3)]
    rows += [dict(vote("f", "T/X/P05-r1", "missing-arm", "-", ""), pref="", s1="", s2="")]
    saved, read_judgments = read_judgments, lambda: rows
    try:
        got = {target[4:7]: (p["outcome"], p["complete"]) for target, p in pair_results("T/X/").items()}
    finally:
        read_judgments = saved
    return got == {"P01": ("win", True), "P02": (None, False), "P03": (None, False), "P04": ("loss", True),
                   "P05": ("loss", True)}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("setup")
    p = sub.add_parser("build")
    p.add_argument("--arm", required=True)
    p.add_argument("--rev")
    p.add_argument("--force", action="store_true")
    p = sub.add_parser("prepare")
    p.add_argument("--gate", required=True)
    p.add_argument("--arm", required=True)
    p.add_argument("--tasks", default="all", help="all, authoring, edits, S8, S8m, or P01,P02,...")
    p.add_argument("--run", type=int, default=1)
    p.add_argument("--source-run", type=int, default=1, help="run whose assets edit tasks start from")
    p.add_argument("--model", default="sonnet")
    p.add_argument("--effort", default="medium")
    p.add_argument("--force", action="store_true")
    p = sub.add_parser("runs")
    p.add_argument("--gate")
    p.add_argument("--arm")
    p.add_argument("--pending", action="store_true")
    p = sub.add_parser("note")
    p.add_argument("run")
    p.add_argument("pairs", nargs="+")
    p = sub.add_parser("score")
    p.add_argument("--gate")
    p.add_argument("--bin")
    p = sub.add_parser("collect")
    p.add_argument("--gate")
    p = sub.add_parser("report")
    p.add_argument("--gate")
    p.add_argument("--vs", help="baseline gate/arm for token ratios and acceptance, e.g. GA/A")
    p = sub.add_parser("calib")
    p.add_argument("--bin")
    p.add_argument("--report", action="store_true")
    p = sub.add_parser("votes")
    p.add_argument("--gate", required=True)
    p.add_argument("--arm", required=True)
    p.add_argument("--bin")
    p = sub.add_parser("pairs")
    p.add_argument("--gate", required=True)
    p.add_argument("--arm", required=True)
    p.add_argument("--vs", default="GA/A", help="baseline gate/arm whose runs each run is paired with")
    p.add_argument("--bin")
    p = sub.add_parser("judgment")
    p.add_argument("image")
    p.add_argument("judge", type=int)
    p.add_argument("answer")
    p.add_argument("--model", default="opus")
    p.add_argument("--effort", default="medium")
    sub.add_parser("selftest")
    args = parser.parse_args()
    globals()[f"cmd_{args.command}"](args)


if __name__ == "__main__":
    main()
