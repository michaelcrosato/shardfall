#!/usr/bin/env python3
"""Plays 52in1's session harness around Shardfall's page, headless (AI Hub Session Protocol v1,
"Test it"; docs/AI_HUB.md). Each case is one harness run; it passes when the harness's
window.__hubCheck.pass is true. The harness files come from 52in1 into .cache/harness.

usage: python3 -P scripts/hub_check.py PAGE.html [CASE ...]   (-P, not -I: Playwright is a user package)

A CASE is the harness's query without game= and auto=, e.g. "mode=moments&players=2" or
"mode=shared-world&how=versus&players=3&latency=150&jitter=50&loss=5". With none, it runs the
hub's own set for every online mode the page's hub:multiplayer names. Software WebGPU
(SwiftShader) is slow: a run takes minutes. Exits 1 when a case fails.
"""
import functools
import http.server
import pathlib
import re
import shutil
import subprocess
import sys
import threading
import time

from playwright.sync_api import sync_playwright

ROOT = pathlib.Path(__file__).resolve().parent.parent / ".cache" / "harness"
NET = "latency=150&jitter=50&loss=5"
FLAGS = ["--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-vulkan=swiftshader", "--use-angle=swiftshader"]


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, format, *args):  # noqa: A002 - the base class names it so
        pass


def harness_files(page):
    ROOT.mkdir(parents=True, exist_ok=True)
    for f in ("harness.html", "bridge.js", "round.js"):
        # curl: the site turns Python's own client away.
        subprocess.run(["curl", "-fsSL", "-o", str(ROOT / f), f"https://52in1.com/lib/hub/{f}"], check=True)
    shutil.copyfile(page, ROOT / "game.html")


def default_cases(html):
    """The hub's own set (its scripts/hub-check.mjs), for the modes hub:multiplayer names."""
    m = re.search(r'<meta name="hub:multiplayer" content="([^"]*)"', html)
    cases = []
    for entry in (m.group(1) if m else "").split(","):
        words = entry.split()
        if not words:
            continue
        where = words[0]
        if where == "parallel":
            cases += [f"mode=parallel&players=1&{NET}", f"mode=parallel&players=2&{NET}"]
        elif where == "moments":
            cases += [f"mode=moments&players=1&{NET}", f"mode=moments&players=2&{NET}"]
        elif where == "ghost":
            cases += [f"mode=ghost&players=2&{NET}", f"mode=ghost&players=3&{NET}&scenario=host-leave"]
        elif where == "shared-world":
            how = words[1]
            cases += [f"mode=shared-world&how={how}&players=2&{NET}", f"mode=shared-world&how={how}&players=2&scenario=server"]
    return cases


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    page, asked = sys.argv[1], sys.argv[2:]
    harness_files(page)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Quiet, directory=str(ROOT)))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    port = server.server_address[1]
    cases = asked or default_cases(pathlib.Path(page).read_text(encoding="utf-8", errors="replace")[:600_000])
    failed = 0
    with sync_playwright() as pw:
        browser = pw.chromium.launch(args=FLAGS)
        for case in cases:
            ctx = browser.new_context(viewport={"width": 1280, "height": 900})
            p = ctx.new_page()
            started = time.time()
            p.goto(f"http://127.0.0.1:{port}/harness.html?game=game.html&auto=1&{case}")
            try:
                p.wait_for_function("document.body.dataset.complete === 'true'", timeout=20 * 60 * 1000, polling=2000)
                report = p.evaluate("({ pass: window.__hubCheck.pass, checks: window.__hubCheck.checks })")
            except Exception as e:  # noqa: BLE001 - a run that never finishes is a failure to report
                report = {"pass": False, "checks": [{"name": "harness finished", "pass": False, "detail": str(e)[:300]}]}
            secs = round(time.time() - started)
            print(f"{'PASS' if report['pass'] else 'FAIL'} {case} ({secs} s, {len(report['checks'])} checks)", flush=True)
            for k in report["checks"]:
                if not k["pass"]:
                    print(f"  x {k['name']}{': ' + k['detail'] if k.get('detail') else ''}", flush=True)
            failed += not report["pass"]
            ctx.close()
        browser.close()
    server.shutdown()
    print(f"{len(cases) - failed} of {len(cases)} cases passed")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
