#!/usr/bin/env python3
"""Prototype of scripts/asset_bench/pav: a logging wrapper around `pav repl` for benchmark runs.

usage: benchpav.py RUN_DIR [--bin PATH] [--cap N] < commands
Lines are pav REPL lines (`tool k=v` or {"tool":..,"args":..}). Each command is sent to one
persistent `pav repl` process (cwd RUN_DIR, PAV_ASSETS=RUN_DIR/props, PAV_CREATURES=RUN_DIR/creatures),
its reply is printed unchanged, and RUN_DIR/log.jsonl gets {i,line,reply,ms,img}. The command count
persists in RUN_DIR/count; past --cap the wrapper refuses commands.
"""
import json, os, subprocess, sys, time

def main():
    argv = sys.argv[1:]
    run = os.path.abspath(argv[0])
    binp = argv[argv.index("--bin") + 1] if "--bin" in argv else "pav"
    cap = int(argv[argv.index("--cap") + 1]) if "--cap" in argv else 30
    os.makedirs(os.path.join(run, "props"), exist_ok=True)
    os.makedirs(os.path.join(run, "creatures"), exist_ok=True)
    env = dict(os.environ, PAV_ASSETS=os.path.join(run, "props"), PAV_CREATURES=os.path.join(run, "creatures"))
    countf = os.path.join(run, "count")
    n, inv = (map(int, open(countf).read().split()) if os.path.exists(countf) else (0, 0))
    inv += 1
    p = subprocess.Popen([binp, "repl"], cwd=run, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True, bufsize=1)
    log = open(os.path.join(run, "log.jsonl"), "a")
    for line in sys.stdin:
        line = line.rstrip("\n")
        if not line.strip() or line.strip().startswith("#"):
            continue
        if n >= cap:
            reply = json.dumps({"error": f"bench cap reached ({cap} commands): stop and report"})
            print(reply, flush=True)
            log.write(json.dumps({"i": n, "line": line, "reply": reply, "ms": 0, "img": None, "refused": True, "inv": inv}) + "\n")
            continue
        t = time.time()
        p.stdin.write(line + "\n")
        p.stdin.flush()
        reply = p.stdout.readline().rstrip("\n")
        ms = int((time.time() - t) * 1000)
        img = None
        try:
            v = json.loads(reply)
            if isinstance(v, dict) and "path" in v and "width" in v and "height" in v:
                img = [v["width"], v["height"]]
        except ValueError:
            pass
        print(reply, flush=True)
        log.write(json.dumps({"i": n, "line": line, "reply": reply, "ms": ms, "img": img, "inv": inv}) + "\n")
        n += 1
    p.stdin.close()
    p.wait()
    open(countf, "w").write(f"{n} {inv}")

if __name__ == "__main__":
    main()
