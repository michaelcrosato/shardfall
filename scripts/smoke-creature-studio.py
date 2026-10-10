#!/usr/bin/env python3
"""Exercise Creature Studio through a real native window and the local MCP bridge.

Build the pinned Node bundle, pav_app and pav_tools first. This script needs a
display or Xvfb. Source writes use isolated temporary folders. Frame timings are
measured without captures; screenshot time is excluded.
"""
import argparse
import copy
import importlib.util
import json
import math
import os
from pathlib import Path
import shutil
import socket
import statistics
import subprocess
import sys
import tempfile
import time

REPO = Path(__file__).resolve().parents[1]
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("animation_smoke", REPO / "scripts/smoke-animation-studio.py")
helpers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helpers)
Mcp = helpers.Mcp


def until(check, timeout=40):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.04)
    raise AssertionError("The expected asynchronous result did not arrive before the deadline")


def wait_job(client, queued, expected="published"):
    assert queued["state"] == "queued" and "feedback" not in queued, queued

    def finished():
        status = client.call("creature_status", {"job": queued["job"]})
        return status if status["state"] in ("published", "failed", "superseded") else None

    status = until(finished, 120)
    assert status["state"] == expected, status
    return status


def wait_frame(client, result):
    ticket = result["feedback"]["ticket"]

    def submitted():
        feedback = client.call("studio_status", {"ticket": ticket})["feedback"]
        assert feedback["state"] in ("pending", "submitted"), feedback
        return feedback if feedback["state"] == "submitted" else None

    return until(submitted, 40)


def summary(values):
    values = sorted(values)
    return {"samples": len(values), "median_ms": statistics.median(values),
            "p95_ms": values[max(0, math.ceil(len(values) * 0.95) - 1)],
            "min_ms": values[0], "max_ms": values[-1]}


def run(game_binary, pav, out, env, samples):
    with socket.socket() as available:
        available.bind(("127.0.0.1", 0))
        address = f"127.0.0.1:{available.getsockname()[1]}"
    checks, timings = [], []

    def passed(message):
        checks.append(message)
        print("PASS: " + message, flush=True)

    def edit(client, args, expected="published"):
        queued = client.call("creature_edit", args)
        result = wait_job(client, queued, expected)
        if expected == "published":
            assert "preview_error" not in result, result
        return result

    with (out / "studio.log").open("w") as log:
        game = subprocess.Popen([str(game_binary), "--creature-studio", "--scene", "empty", "--bridge", address],
                                cwd=REPO, env=env, stdout=log, stderr=subprocess.STDOUT)
        client = None
        try:
            helpers.wait_for_bridge(game, address)
            client = Mcp(pav, env, address)
            tools = client.request("tools/list", {})["tools"]
            assert {"creature_catalog", "creature_edit", "creature_preview", "creature_status", "studio_status"} <= {tool["name"] for tool in tools}
            catalog = client.call("creature_catalog")
            assert catalog["generator_revision"] == "851880256987ecdb2895c6afd01f84df64199bdb", catalog
            assert len(catalog["modules"]) >= 80 and len(catalog["templates"]) >= 8, catalog
            initial = client.call("studio_status")
            assert initial["mode"] == "creature" and not client.call("creature_preview")["available"], initial
            world_tick = client.call("status")["tick"]
            passed("MCP discovers the pinned compiler catalog and opens an empty Creature stage")

            created = edit(client, {"action": "create", "name": "smoke_ridgeback", "template": "ridgeback_stalker",
                                    "quality": "medium", "if_revision": "absent"})
            wait_frame(client, created)
            name, source = created["name"], Path(created["file"])
            assert source.is_file() and created["saved"], created
            view = client.call("creature_preview", {"clip": "walk", "time": 0.12, "playing": False})
            wait_frame(client, view)
            before_pose = client.call("creature_preview", {"action": "pose"})
            camera = client.call("params", {"prefix": "camera"})
            client.call("capture", {"width": 800, "height": 600, "out": str(out / "creature-before.png")},
                        image=out / "creature-before.png")
            assert view["bones"] == 71 and {"walk", "trot", "bite", "roar", "death"} <= set(view["clips"]), view
            passed("A template compiles, saves, and reaches a real native frame with six baked clips")

            view = client.call("creature_preview", {"time": view["duration"] * 0.6, "playing": False})
            paused_time = view["time"]
            wait_frame(client, view)
            moved_pose = client.call("creature_preview", {"action": "pose"})
            assert moved_pose["joints"] != before_pose["joints"], moved_pose
            client.call("capture", {"width": 800, "height": 600, "out": str(out / "creature-walk.png")},
                        image=out / "creature-walk.png")
            assert (out / "creature-before.png").read_bytes() != (out / "creature-walk.png").read_bytes()
            passed("Baked motion changes inspected joints and the native rendered image")

            changed = edit(client, {"action": "patch", "name": name, "if_revision": created["revision"], "ops": [
                {"op": "set", "path": "parts[id=horns].params.length", "value": 0.42},
                {"op": "set", "path": "skin.palette.base", "value": "#255e73"},
                {"op": "set", "path": "skin.layers[type=stripes].count", "value": 8},
            ]})
            wait_frame(client, changed)
            current = client.call("creature_preview")
            assert current["source_revision"] == changed["revision"]
            assert math.isclose(current["time"], paused_time, abs_tol=1e-6) and not current["playing"], {"expected_time": paused_time, "current": current, "build": changed}
            assert client.call("params", {"prefix": "camera"}) == camera
            client.call("capture", {"width": 800, "height": 600, "out": str(out / "creature-after.png")},
                        image=out / "creature-after.png")
            assert (out / "creature-walk.png").read_bytes() != (out / "creature-after.png").read_bytes()
            passed("One named part/surface batch updates the image and preserves paused time and camera")

            saved = source.read_bytes()
            failed = edit(client, {"action": "patch", "name": name, "if_revision": changed["revision"], "ops": [
                {"op": "set", "path": "body.tail.length", "value": -3}
            ]}, "failed")
            assert "feedback" not in failed
            client.call("creature_edit", {"action": "patch", "name": name, "if_revision": created["revision"],
                        "ops": [{"op": "set", "path": "body.tail.length", "value": 0.3}]}, fail=True)
            assert source.read_bytes() == saved
            assert client.call("creature_preview")["source_revision"] == changed["revision"]
            passed("Compiler failures and stale revisions preserve the saved source and last valid preview")

            undone = edit(client, {"action": "undo", "name": name, "if_revision": changed["revision"]})
            assert undone["revision"] == created["revision"]
            wait_frame(client, undone)
            redone = edit(client, {"action": "redo", "name": name, "if_revision": undone["revision"]})
            assert redone["revision"] == changed["revision"]
            wait_frame(client, redone)
            passed("Undo and redo restore exact source revisions and distinct submitted-frame tickets")

            # A heavy request leaves enough time to enqueue its replacement in the live app.
            old = client.call("creature_edit", {"action": "patch", "name": name, "if_revision": redone["revision"],
                              "quality": "high", "ops": [{"op": "set", "path": "body.tail.length", "value": 1.7}]})
            latest = client.call("creature_edit", {"action": "patch", "name": name, "if_revision": redone["revision"],
                                 "quality": "medium", "ops": [{"op": "set", "path": "body.tail.length", "value": 1.1}]})
            wait_job(client, old, "superseded")
            current = wait_job(client, latest)
            wait_frame(client, current)
            inspected = client.call("creature_edit", {"action": "inspect", "name": name})
            assert inspected["blueprint"]["body"]["tail"]["length"] == 1.1
            passed("A newer request supersedes an in-flight build without publishing its obsolete result")

            revision = current["revision"]
            for index in range(samples):
                start = time.perf_counter()
                queued = client.call("creature_edit", {"action": "patch", "name": name, "if_revision": revision,
                    "ops": [{"op": "set", "path": "skin.layers[type=countershade].strength", "value": 0.4 if index % 2 else 0.7}]})
                queued_ms = (time.perf_counter() - start) * 1000
                result = wait_job(client, queued)
                feedback = wait_frame(client, result)
                timings.append({"queued_rpc_ms": queued_ms, "build_ms": result["build_ms"],
                                "compile_ms": result.get("compile_ms"), "build_kind": result.get("build_kind"),
                                "submitted_ms": feedback["submitted_ms"],
                                "confirmed_ms": (time.perf_counter() - start) * 1000})
                revision = result["revision"]
            passed(f"{samples} ordinary surface edits publish and receive native frame confirmations")

            edited_file = json.loads(source.read_text())
            edited_file["blueprint"]["skin"]["palette"]["base"] = "#496b37"
            source.write_text(json.dumps(edited_file, indent=2) + "\n")
            reloaded = until(lambda: (state if (state := client.call("creature_preview"))["source_revision"] != revision else None))
            revision = reloaded["source_revision"]
            invalid = copy.deepcopy(edited_file)
            invalid["blueprint"]["body"]["tail"]["length"] = -7
            source.write_text(json.dumps(invalid) + "\n")
            until(lambda: any(job["state"] == "failed" and job.get("action") == "reload"
                              for job in client.call("creature_status")["jobs"]))
            assert client.call("creature_preview")["source_revision"] == revision
            source.write_text(json.dumps(edited_file, indent=2) + "\n")
            passed("Debounced file reloads adopt valid source and retain the last good creature after invalid edits")

            creature_state = client.call("creature_preview")
            creature_camera = client.call("params", {"prefix": "camera"})
            client.call("anim_preview", {"clip": "QUATERNIUS/Idle_Loop", "time": 0.4, "playing": False})
            client.call("asset_preview", {"name": "BUILTIN/bench", "playing": False})
            client.call("creature_preview", {"action": "open"})
            restored = client.call("creature_preview")
            assert restored["name"] == name and restored["time"] == creature_state["time"]
            assert client.call("params", {"prefix": "camera"}) == creature_camera
            assert client.call("status")["tick"] == world_tick
            passed("Animations, Objects and Creatures retain their clocks and cameras while the game stays paused")

            cold_env = dict(env, PAV_NODE="missing-node-intentionally-for-cache-test")
            cold = Mcp(pav, cold_env)
            try:
                cold_source = cold.call("creature_edit", {"action": "inspect", "name": name})
                assert cold_source["compiled"] and cold_source["revision"] == revision, cold_source
                assert cold.call("creature_preview", {"name": name})["available"]
                assert cold.call("studio_status")["feedback"]["available"] is False
            finally:
                cold.close()
            passed("A new MCP process reloads the saved native cache without starting Node")

            existing = client.call("creature_edit", {"action": "inspect", "name": name})
            from_blueprint = edit(client, {"action": "create", "name": "smoke_blueprint",
                "blueprint": existing["blueprint"], "quality": "low", "if_revision": "absent"})
            wait_frame(client, from_blueprint)
            copied = edit(client, {"action": "copy", "name": "smoke_copy", "from": from_blueprint["name"],
                                   "if_revision": "absent"})
            wait_frame(client, copied)
            copy_source = client.call("creature_edit", {"action": "inspect", "name": copied["name"]})
            original_source = client.call("creature_edit", {"action": "inspect", "name": from_blueprint["name"]})
            assert copied["file"] != from_blueprint["file"] and copy_source["blueprint"] == original_source["blueprint"]
            passed("An existing blueprint creates a native asset, and copy saves an independent source")

            themed = edit(client, {"action": "create", "name": "smoke_theme", "theme": "beast",
                                   "seed": 51, "constraints": {"bodyPlan": "quadruped"}, "quality": "low"})
            assert themed["saved"]
            removed = edit(client, {"action": "undo", "name": themed["name"], "if_revision": themed["revision"]})
            assert removed["revision"] == "absent"
            assert client.call("studio_status")["mode"] == "creature"
            assert not client.call("creature_preview")["available"]
            recovered = edit(client, {"action": "redo", "name": themed["name"], "if_revision": "absent"})
            assert recovered["revision"] == themed["revision"]
            passed("Theme creation supports undo creation, an empty stage, and redo")

            winged = edit(client, {"action": "create", "name": "smoke_bat", "template": "cave_bat", "quality": "low"})
            wait_frame(client, winged)
            flight = client.call("creature_preview", {"clip": "fly", "time": 0.2, "playing": False})
            wait_frame(client, flight)
            client.call("capture", {"width": 800, "height": 600, "out": str(out / "creature-flight.png")},
                        image=out / "creature-flight.png")
            assert "fly" in flight["clips"]
            passed("A winged creature previews with baked flight and double-sided native membranes")

            final = client.call("creature_preview", {"name": name, "clip": "walk", "time": 0.24, "playing": False})
            wait_frame(client, final)
            if shutil.which("import") and os.environ.get("DISPLAY"):
                subprocess.run(["import", "-window", "root", str(out / "studio-window.png")], check=True, timeout=30)
            close = client.call("creature_preview", {"action": "close"})
            wait_frame(client, close)
            assert client.call("studio_status")["mode"] == "world" and game.poll() is None
            passed("The real window submits the final Creature panel and returns to Game")

            result = {"passed": len(checks), "checks": checks,
                      "latency": {key: summary([row[key] for row in timings])
                                  for key in ("queued_rpc_ms", "build_ms", "submitted_ms", "confirmed_ms")},
                      "samples": timings,
                      "measurement": "Local MCP, native Vulkan window; frame submission excludes physical display latency. No captures during latency samples.",
                      "images": sorted(path.name for path in out.glob("*.png"))}
            (out / "results.json").write_text(json.dumps(result, indent=2) + "\n")
            print(json.dumps(result, indent=2), flush=True)
        finally:
            if client:
                client.close()
            game.terminate()
            try:
                game.wait(timeout=10)
            except subprocess.TimeoutExpired:
                game.kill()
                game.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    suffix = ".exe" if os.name == "nt" else ""
    parser.add_argument("--game", type=Path, default=REPO / "target/debug" / ("shardfall" + suffix))
    parser.add_argument("--pav", type=Path, default=REPO / "target/debug" / ("pav" + suffix))
    parser.add_argument("--out", type=Path, default=REPO / "out/creature-studio-smoke")
    parser.add_argument("--samples", type=int, default=5)
    args = parser.parse_args()
    if args.samples < 1:
        parser.error("samples must be positive")
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="pav-creature-smoke-") as folder:
        env = dict(os.environ, PAV_CREATURES=str(Path(folder) / "creatures"),
                   PAV_ASSETS=str(Path(folder) / "props"), PAV_ANIM=str(Path(folder) / "anim"))
        run(args.game.resolve(), args.pav.resolve(), out, env, args.samples)


if __name__ == "__main__":
    main()
