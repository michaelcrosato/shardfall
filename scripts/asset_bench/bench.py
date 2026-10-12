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
  bench.py report [--gate GA]                      gate summary -> results/summary.json
  bench.py selftest                                two scripted replays: equal tok_x, cap enforced

Runs live in .cache/asset_bench/runs/<gate>/<arm>/<task>-r<k>/ (git-ignored). Each author run is
one fresh `asset-bench-author` agent (.claude/agents/) that reads RUN/brief.md and RUN/docs/ and
drives the engine only through scripts/asset_bench/pav RUN.
"""

import argparse
import json
import math
import os
import re
import shutil
import statistics
import subprocess
import sys
import time
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BENCH = REPO / "scripts/asset_bench"
CACHE = REPO / ".cache/asset_bench"
RUNS, BIN, VENV = CACHE / "runs", CACHE / "bin", CACHE / "venv"
RESULTS = BENCH / "results"
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
        json.dumps({"tool": "asset_edit", "args": {"action": "check", "name": f"WORKSHOP/{name}"}})])
    inspect = replies[0] if replies else {"error": "no reply"}
    facts = {"loads": bool(inspect.get("asset")), "problems": []}
    if not facts["loads"]:
        facts["problems"].append(f"{name}: does not load ({str(inspect.get('error', 'absent'))[:120]})")
        return facts
    lo, hi = inspect["bounds"]
    cm = [round((hi[i] - lo[i]) * 100, 1) for i in range(3)]
    facts.update(parts=len(inspect["asset"]["parts"]), size_cm="x".join(f"{c:g}" for c in cm))
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
    if "light" in needs:
        parts = inspect["asset"]["parts"].values()
        if not any(p.get("emissive", 0) > 0.5 or p.get("light") for p in parts):
            facts["problems"].append(f"{name}: gives off no light")
    check = replies[1] if len(replies) > 1 else {}
    if "error" in check:
        facts["check"] = "na"  # the arm's engine has no lint yet; re-scored when it does
    else:
        facts["check"] = "ok"
        blocking = {"floating", "unit_suspect"}
        if "hang" not in needs and "wall" not in needs:
            blocking |= {"below_floor", "not_grounded"}
        for line in check.get("check", []):
            if line.split(":")[0] in blocking:
                facts["problems"].append(f"{name}: {line}")
    return facts


def cmd_score(args):
    ensure_tokenizer()
    enc = encoder()
    binary = Path(args.bin) if args.bin else (BIN / "pav-HEAD" if (BIN / "pav-HEAD").exists()
                                               else REPO / "target/debug/pav")
    table = tasks()
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
        problems, parts, sizes, check = [], 0, [], "na"
        for name in config["names"]:
            facts = check_asset(binary, run, name, task.get("needs", []), task.get("size"), task.get("tol", 0.15))
            problems += facts["problems"]
            parts += facts.get("parts", 0)
            sizes.append(facts.get("size_cm", "-"))
            check = facts.get("check", check)
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
            "first_ok": "" if first_ok is None else int(first_ok), "lint_block": "na" if check == "na" else "",
            "lint_other": "na" if check == "na" else "", "check": check, "wall_s": wall,
            "engine_ms": sum(r["ms"] for r in logged), "parts": parts, "tris": "na", "size_cm": "/".join(sizes),
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
    with open(RESULTS / "summary.json", "w") as f:
        json.dump(summary, f, indent=1, sort_keys=True)
        f.write("\n")


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
    print("selftest " + ("passed" if ok else "FAILED"))
    sys.exit(0 if ok else 1)


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
    sub.add_parser("selftest")
    args = parser.parse_args()
    globals()[f"cmd_{args.command}"](args)


if __name__ == "__main__":
    main()
