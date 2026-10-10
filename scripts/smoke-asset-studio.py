#!/usr/bin/env python3
"""Check live object authoring through a real desktop window and MCP server.

Build pav_app and pav_tools first. Requires a display or Xvfb. All authored assets
use temporary folders. Reports include ordinary edit and frame-submission timings;
captures are measured separately and do not run during the edit sample.
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


def wait_submission(client, result, timeout=25):
    ticket = result["feedback"]["ticket"]
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        status = client.call("studio_status", {"ticket": ticket})["feedback"]
        if status["state"] == "submitted":
            return status
        assert status["state"] == "pending", status
        time.sleep(0.01)
    raise AssertionError(f"The window did not submit edit ticket {ticket}: {status}")


def until(check, timeout=12):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.05)
    raise AssertionError("A live file update did not arrive before the deadline")


def summary(values):
    ordered = sorted(values)
    return {"samples": len(values), "median_ms": statistics.median(values),
            "p95_ms": ordered[max(0, math.ceil(len(values) * 0.95) - 1)],
            "min_ms": ordered[0], "max_ms": ordered[-1]}


def run(game_binary, pav, out, env, samples):
    with socket.socket() as available:
        available.bind(("127.0.0.1", 0))
        addr = f"127.0.0.1:{available.getsockname()[1]}"
    checks = []

    def passed(message):
        checks.append(message)
        print(f"PASS: {message}", flush=True)

    with (out / "studio.log").open("w") as log:
        game = subprocess.Popen([str(game_binary), "--asset-studio", "--scene", "level/1", "--bridge", addr],
                                cwd=REPO, env=env, stdout=log, stderr=subprocess.STDOUT)
        client = None
        try:
            helpers.wait_for_bridge(game, addr)
            client = Mcp(pav, env, addr)
            listed = client.request("tools/list", {})["tools"]
            assert {"assets", "asset_edit", "asset_preview", "asset_spawn", "studio_status", "anim_edit", "anim_preview"} <= {t["name"] for t in listed}
            catalog = client.call("assets")
            assert {"BUILTIN/box", "BUILTIN/crate", "BUILTIN/bench", "BUILTIN/lantern"} <= {a["name"] for a in catalog["assets"]}
            initial_world = client.call("status")
            assert initial_world["scene"] == "level/1"
            passed("MCP discovers object tools and all four templates over the live bridge")

            custom = {"format": 1, "name": "marker", "description": "Created from scratch by MCP.", "parts": {
                "post": {"shape": {"type": "cylinder", "half_height": 0.6, "radius": 0.05}, "pos": [0, 0.6, 0], "color": "#26313e"},
                "light": {"shape": {"type": "sphere", "radius": 0.16}, "pos": [0, 1.3, 0], "color": "#79debd", "emissive": 2, "solid": False},
            }}
            made = client.call("asset_edit", {"action": "create", "name": "marker", "asset": custom, "preview": False})
            assert made["saved"] and Path(made["file"]).is_file(), made
            assert client.call("asset_edit", {"action": "inspect", "name": "marker"})["asset"]["parts"].keys() == custom["parts"].keys()
            passed("An agent creates and saves an object from a complete text definition")

            created = client.call("asset_edit", {"action": "copy", "from": "BUILTIN/bench", "name": "garden_bench"})
            name, path = created["name"], Path(created["file"])
            assert created["saved"] and "preview_error" not in created, created
            paused = client.call("asset_preview", {"time": 0, "playing": False})
            wait_submission(client, paused)
            camera = client.call("params", {"prefix": "camera"})
            client.call("capture", {"width": 720, "height": 540, "out": str(out / "bench-before.png")}, image=out / "bench-before.png")

            first = client.call("asset_spawn", {"action": "add", "name": name, "pos": [3, 0, 3], "yaw": 30})["instance"]
            second = client.call("asset_spawn", {"action": "add", "name": name, "pos": [7, 0, 3], "scale": 0.8, "collide": False})["instance"]
            ops = [
                {"op": "set", "part": "seat", "fields": {"color": "#3e9a9e"}},
                {"op": "set", "part": "backrest", "fields": {"color": "#3e9a9e"}},
            ]
            for part in ["leg_front_left", "leg_front_right", "leg_back_left", "leg_back_right", "crossbar"]:
                ops.append({"op": "set", "part": part, "fields": {"color": "#26313e"}})
            for side, x in [("left", -1.0), ("right", 1.0)]:
                ops.append({"op": "add", "part": "armrest_" + side, "value": {
                    "shape": {"type": "rounded_box", "half": [0.07, 0.06, 0.42], "radius": 0.025},
                    "pos": [x, 1.12, 0], "color": "#d4a45c",
                }})
                ops.append({"op": "add", "part": "arm_support_" + side, "value": {
                    "shape": {"type": "box", "half": [0.045, 0.1, 0.045]},
                    "pos": [x, 0.99, 0.3], "color": "#26313e",
                }})
            changed = client.call("asset_edit", {"action": "patch", "name": name, "ops": ops, "if_revision": created["revision"]})
            assert changed["refreshed_instances"] == 2 and "preview_error" not in changed, changed
            wait_submission(client, changed)
            preview = client.call("asset_preview")
            assert preview["time"] == 0 and not preview["playing"]
            assert client.call("params", {"prefix": "camera"}) == camera
            instances = client.call("asset_spawn", {"action": "list", "name": name})["instances"]
            for old in [first, second]:
                current = next(i for i in instances if i["id"] == old["id"])
                for key in ["pos", "yaw", "scale", "collide"]:
                    assert current[key] == old[key], (key, current, old)
                assert current["revision"] == changed["revision"]
            client.call("capture", {"width": 720, "height": 540, "out": str(out / "bench-after.png")}, image=out / "bench-after.png")
            assert (out / "bench-before.png").read_bytes() != (out / "bench-after.png").read_bytes()
            assert client.call("status")["tick"] == initial_world["tick"]
            passed("One paused batch changes the rendered object and two placed copies, preserving camera and world tick")

            saved = path.read_bytes()
            bad_ops = [{"op": "set", "part": "seat", "fields": {"color": "#ffffff"}}, {"op": "remove", "part": "missing"}]
            client.call("asset_edit", {"action": "patch", "name": name, "ops": bad_ops}, fail=True)
            client.call("asset_edit", {"action": "patch", "name": name, "ops": [], "if_revision": created["revision"]}, fail=True)
            client.call("asset_edit", {"action": "patch", "name": name, "ops": [], "preview": "invalid"}, fail=True)
            client.call("asset_preview", {"name": name, "colour": "#ffffff"}, fail=True)
            assert path.read_bytes() == saved
            assert client.call("asset_preview")["revision"] == changed["revision"]
            passed("Bad batches, stale revisions, and bad options leave disk and live geometry unchanged")

            undone = client.call("asset_edit", {"action": "undo", "name": name, "if_revision": changed["revision"]})
            assert undone["revision"] == created["revision"] and undone["refreshed_instances"] == 2
            wait_submission(client, undone)
            redone = client.call("asset_edit", {"action": "redo", "name": name, "if_revision": undone["revision"]})
            assert redone["revision"] == changed["revision"]
            wait_submission(client, redone)
            passed("Undo and redo restore exact revisions and receive distinct submitted-frame tickets")

            # The warmed path: small accepted batches over one persistent MCP connection.
            # Image rendering and encoding do not run during this sample.
            rpc_ms, submitted_ms, apply_ms, confirmed_ms = [], [], [], []
            revision = redone["revision"]
            for index in range(samples):
                started = time.perf_counter()
                edited = client.call("asset_edit", {"action": "patch", "name": name, "if_revision": revision,
                                      "ops": [{"op": "set", "part": "seat", "fields": {"color": "#3e9a9e" if index % 2 else "#459fa3"}}]})
                rpc_ms.append((time.perf_counter() - started) * 1000)
                status = wait_submission(client, edited)
                confirmed_ms.append((time.perf_counter() - started) * 1000)
                submitted_ms.append(status["submitted_ms"])
                apply_ms.append(status["apply_ms"])
                revision = edited["revision"]
            latency = {"environment": "native window; renderer/adapter in studio.log", "rpc_return": summary(rpc_ms),
                       "tool_application": summary(apply_ms), "tool_start_to_frame_submission": summary(submitted_ms),
                       "rpc_start_to_confirmation": summary(confirmed_ms),
                       "note": "RPC and confirmation include local MCP/bridge transport and status polling. Submission excludes transport and physical display; no captures during sample."}
            passed(f"{samples} ordinary edits are saved and confirmed by actual frame submissions")

            # A direct file edit must update both preview and instances, even if a catalog
            # read happens before the watcher. Catalog reads must not steal reload events.
            valid = json.loads(path.read_text())
            valid["parts"]["backrest"]["color"] = "#68b7ba"
            path.write_text(json.dumps(valid, indent=2) + "\n")
            client.call("assets")
            external = until(lambda: (p if (p := client.call("asset_preview"))["revision"] != revision else None))
            def refreshed_instances():
                items = client.call("asset_spawn", {"action": "list", "name": name})["instances"]
                return items if items and all(i["revision"] == external["revision"] for i in items) else None
            listed_instances = until(refreshed_instances)
            assert all(i["revision"] == external["revision"] for i in listed_instances)
            invalid = copy.deepcopy(valid)
            invalid["parts"]["seat"]["shape"]["half"][0] = -1
            path.write_text(json.dumps(invalid) + "\n")
            time.sleep(1.0)
            assert client.call("asset_preview")["revision"] == external["revision"]
            path.write_text(json.dumps(valid, indent=2) + "\n")
            passed("Late file watching applies valid external edits to placed copies and retains the last valid object after an error")

            client.call("anim_preview", {"clip": "QUATERNIUS/Idle_Loop", "time": 0.4, "playing": False})
            assert client.call("studio_status")["mode"] == "animation"
            client.call("asset_preview", {"action": "open"})
            assert client.call("asset_preview")["name"] == name
            assert client.call("asset_preview")["time"] == 0
            client.call("anim_preview", {"action": "open"})
            assert math.isclose(client.call("anim_preview")["time"], 0.4, abs_tol=1e-6)
            client.call("asset_preview", {"action": "open"})
            passed("Animation and object tabs retain their selections and paused clocks")

            moved = client.call("asset_spawn", {"action": "update", "id": second["id"], "pos": [8, 0, 4], "yaw": 70})
            assert moved["instance"]["pos"] == [8, 0, 4] and moved["instance"]["id"] == second["id"]
            client.call("asset_spawn", {"action": "remove", "id": second["id"]})
            remaining = client.call("asset_spawn", {"action": "list", "name": name})["instances"]
            assert len(remaining) == 1 and remaining[0]["id"] == first["id"]
            assert client.call("status")["tick"] == initial_world["tick"]
            passed("Placed root objects move and delete without advancing the preserved game")

            cold = Mcp(pav, env)
            try:
                persisted = cold.call("asset_edit", {"action": "inspect", "name": name})
                assert persisted["asset"]["parts"]["backrest"]["color"] == "#68b7ba"
                assert cold.call("asset_preview", {"name": name})["available"]
                assert cold.call("studio_status")["feedback"]["available"] is False
            finally:
                cold.close()
            passed("A new headless MCP process loads and previews saved objects")

            example = subprocess.run([str(pav), "live", addr, "--stop-on-error"],
                                     input=(REPO / "scripts/examples/object-lantern.jsonl").read_text(),
                                     cwd=REPO, env=env, text=True, capture_output=True, timeout=45)
            assert example.returncode == 0, (example.stdout, example.stderr)
            assert client.call("asset_preview")["name"] == "WORKSHOP/garden_lantern"
            passed("The shipped lantern example runs through the same live CLI")

            final = client.call("asset_preview", {"name": name, "playing": False, "time": 0})
            wait_submission(client, final)
            client.call("capture", {"width": 720, "height": 540, "out": str(out / "bench-final.png")}, image=out / "bench-final.png")
            if shutil.which("import") and os.environ.get("DISPLAY"):
                # Submission is acknowledged above, so no fixed multi-second screenshot wait.
                subprocess.run(["import", "-window", "root", str(out / "studio-window.png")], check=True, timeout=30)
            closed = client.call("asset_preview", {"close": True})
            assert closed["open"] is False and game.poll() is None
            wait_submission(client, closed)
            passed("The native window submits the final object view and returns to the running game")
            result = {"passed": len(checks), "checks": checks, "latency": latency,
                      "images": sorted(p.name for p in out.glob("*.png"))}
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
    extension = ".exe" if os.name == "nt" else ""
    parser.add_argument("--game", type=Path, default=REPO / "target/debug" / ("shardfall" + extension))
    parser.add_argument("--pav", type=Path, default=REPO / "target/debug" / ("pav" + extension))
    parser.add_argument("--out", type=Path, default=REPO / "out/asset-studio-smoke")
    parser.add_argument("--samples", type=int, default=20)
    args = parser.parse_args()
    if args.samples < 1:
        parser.error("samples must be positive")
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="pav-object-smoke-") as directory:
        env = dict(os.environ, PAV_ASSETS=str(Path(directory) / "props"), PAV_ANIM=str(Path(directory) / "anim"))
        run(args.game.resolve(), args.pav.resolve(), out, env, args.samples)


if __name__ == "__main__":
    main()
