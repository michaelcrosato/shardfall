#!/usr/bin/env python3
"""Check the real desktop studio through MCP. Requires a display (or Xvfb).

Build pav_app and pav_tools first. All test animations use a temporary PAV_ANIM
directory. The repository's animation files are only read.
"""

import argparse
import base64
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time


REPO = Path(__file__).resolve().parents[1]


class Mcp:
    def __init__(self, binary, env, addr=None):
        args = [str(binary), "mcp"] + (["--live", addr] if addr else ["empty"])
        self.process = subprocess.Popen(
            args, cwd=REPO, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, bufsize=1,
        )
        self.next_id = 0
        self.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                    "clientInfo": {"name": "studio-smoke", "version": "1"}})

    def request(self, method, params):
        self.next_id += 1
        request = {"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params}
        self.process.stdin.write(json.dumps(request) + "\n")
        self.process.stdin.flush()
        line = self.process.stdout.readline()
        if not line:
            raise AssertionError("MCP closed: " + self.process.stderr.read())
        reply = json.loads(line)
        assert reply.get("id") == self.next_id, reply
        assert "error" not in reply, reply
        return reply["result"]

    def call(self, name, args=None, fail=False, image=None):
        result = self.request("tools/call", {"name": name, "arguments": args or {}})
        if fail:
            assert result.get("isError"), result
            return result
        assert not result.get("isError"), result
        for content in result.get("content", []):
            if content["type"] == "image" and image:
                assert content["mimeType"] == "image/png"
                png = base64.b64decode(content["data"])
                assert png.startswith(b"\x89PNG\r\n\x1a\n")
                image.write_bytes(png)
        texts = [c["text"] for c in result.get("content", []) if c["type"] == "text"]
        return json.loads(texts[-1]) if texts else result

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()


def wait_for_bridge(game, addr):
    deadline = time.monotonic() + 90
    host, port = addr.rsplit(":", 1)
    while time.monotonic() < deadline:
        if game.poll() is not None:
            raise AssertionError("The game exited before the bridge opened. See studio.log.")
        try:
            with socket.create_connection((host, int(port)), timeout=0.2):
                return
        except OSError:
            time.sleep(0.1)
    raise AssertionError("The game did not open its bridge within 90 seconds.")


def check(studio, pav, out, env):
    with socket.socket() as port:
        port.bind(("127.0.0.1", 0))
        addr = f"127.0.0.1:{port.getsockname()[1]}"
    with (out / "studio.log").open("w") as log:
        game = subprocess.Popen([str(studio), "--animation-studio", "--bridge", addr],
                                cwd=REPO, env=env, stdout=log, stderr=subprocess.STDOUT)
        client = None
        try:
            wait_for_bridge(game, addr)
            client = Mcp(pav, env, addr)
            tools = client.request("tools/list", {})["tools"]
            assert {"anim_edit", "anim_preview"} <= {t["name"] for t in tools}
            checks = ["MCP initializes and exposes animation tools"]

            created = client.call("anim_edit", {"action": "create", "name": "SmokeWave", "duration": 2})
            assert created["saved"] and "preview_error" not in created, created
            name = created["name"]
            path = Path(created["file"])
            assert path.is_file()
            client.call("anim_preview", {"time": 0.5, "playing": False})
            before = client.call("anim_preview", {"action": "pose"})
            client.call("capture", {"width": 480, "height": 480, "out": str(out / "rest.png")}, image=out / "rest.png")

            changed = client.call("anim_edit", {"action": "key", "name": name, "time": 0.5,
                                  "pose": {"armR": [80, 20, 50, 25, 0]}, "if_revision": created["revision"]})
            assert changed["revision"] != created["revision"] and "preview_error" not in changed, changed
            pose = client.call("anim_preview", {"action": "pose"})
            assert pose["time"] == 0.5 and pose["playing"] is False, pose
            assert pose["key"]["armR"] == [80, 20, 50, 25, 0], pose
            assert pose["joints"]["handR"] != before["joints"]["handR"]
            client.call("capture", {"width": 480, "height": 480, "out": str(out / "edited.png")}, image=out / "edited.png")
            assert (out / "rest.png").read_bytes() != (out / "edited.png").read_bytes()
            checks.append("Saved pose edits change the rendered image while the preview stays paused at the same time")

            saved = path.read_bytes()
            client.call("anim_edit", {"action": "mirror", "name": name, "if_revision": created["revision"]}, fail=True)
            client.call("anim_edit", {"action": "key", "name": name, "time": 0.5,
                                      "pose": {"armR": [0, 0, 0]}}, fail=True)
            client.call("anim_edit", {"action": "mirror", "name": name, "preview": "invalid"}, fail=True)
            assert path.read_bytes() == saved
            checks.append("Stale revisions and invalid requests leave the saved file unchanged")

            undone = client.call("anim_edit", {"action": "undo", "name": name, "if_revision": changed["revision"]})
            assert undone["revision"] == created["revision"]
            redone = client.call("anim_edit", {"action": "redo", "name": name, "if_revision": undone["revision"]})
            assert redone["revision"] == changed["revision"]
            checks.append("Undo and redo restore exact revisions through MCP")

            batch = "\n".join(json.dumps(command) for command in [
                {"tool": "anim_edit", "args": {"action": "create", "name": "SmokeWave", "duration": 2}},
                {"tool": "anim_edit", "args": {"action": "retime", "name": name, "factor": 2}},
            ]) + "\n"
            saved = path.read_bytes()
            stopped = subprocess.run([str(pav), "live", addr, "--stop-on-error"], input=batch,
                                     cwd=REPO, env=env, text=True, capture_output=True, timeout=30)
            assert stopped.returncode != 0 and path.read_bytes() == saved, stopped
            checks.append("Batch mode stops after a failed create before it can change an existing animation")

            external = json.loads(path.read_text())
            external["clips"]["SmokeWave"]["keys"][1]["head"] = [35, 0, 0]
            path.write_text(json.dumps(external))
            deadline = time.monotonic() + 8
            while time.monotonic() < deadline:
                pose = client.call("anim_preview", {"action": "pose"})
                if pose["key"]["head"] == [35, 0, 0]:
                    break
                time.sleep(0.15)
            else:
                raise AssertionError("The file watcher did not apply the external pose edit.")
            valid = path.read_bytes()
            path.write_text('{"invalid":')
            time.sleep(0.7)
            assert client.call("anim_preview", {"action": "pose"})["key"]["head"] == [35, 0, 0]
            invalid = json.loads(valid)
            invalid["clips"]["SmokeWave"]["keys"][1]["armR"] = [0, 0, 0, 0, 0]
            path.write_text(json.dumps(invalid))
            time.sleep(0.7)
            held_pose = client.call("anim_preview", {"action": "pose"})
            assert held_pose["key"]["armR"] == [80, 20, 50, 25, 0]
            path.write_bytes(valid)
            checks.append("External file changes appear live; invalid files retain the last valid pose")

            example = subprocess.run([str(pav), "live", addr, "--stop-on-error"],
                                     input=(REPO / "scripts/examples/animation-wave.jsonl").read_text(),
                                     cwd=REPO, env=env, text=True, capture_output=True, timeout=30)
            assert example.returncode == 0, example
            assert client.call("anim_preview")["name"] == "WORKSHOP/HelloWave"
            checks.append("The supplied wave example creates and plays a complete animation through the live CLI")

            client.call("anim_preview", {"time": 0, "playing": True, "speed": 1})
            client.call("filmstrip", {"frames": 8, "every": 12, "columns": 4, "width": 240, "height": 240,
                                      "out": str(out / "filmstrip.png")}, image=out / "filmstrip.png")
            client.call("anim_preview", {"time": 0.5, "playing": False})
            if shutil.which("import") and os.environ.get("DISPLAY"):
                # A bridge reply precedes the window's next present. Give software
                # rendering time to display the paused state before the OS capture.
                time.sleep(2)
                subprocess.run(["import", "-window", "root", str(out / "studio-window.png")], check=True, timeout=30)
            checks.append("MCP returns a filmstrip from the same studio renderer")

            headless = Mcp(pav, env)
            try:
                reopened = headless.call("anim_preview", {"clip": name, "time": 0.5})
                assert reopened["name"] == name and reopened["time"] == 0.5
                assert headless.call("anim_preview", {"action": "pose"})["key"]["head"] == [35, 0, 0]
                persisted = headless.call("anim_edit", {"action": "inspect", "name": name})
                assert persisted["clip"]["keys"][1]["head"] == [35, 0, 0]
            finally:
                headless.close()
            checks.append("A new headless MCP process can preview and read the persisted animation")

            closed = client.call("anim_preview", {"close": True})
            assert closed["open"] is False
            assert game.poll() is None
            checks.append("The preview closes and the game remains running")
            result = {"passed": len(checks), "checks": checks, "images": [p.name for p in out.glob("*.png")]}
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
    parser.add_argument("--out", type=Path, default=REPO / "out/animation-studio-smoke")
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="pav-studio-smoke-") as directory:
        # A bare binary must attach its watcher when the first bridge edit creates
        # the animation directory. Start without one to exercise that path too.
        env = dict(os.environ, PAV_ANIM=str(Path(directory) / "anim"))
        check(args.game.resolve(), args.pav.resolve(), out, env)


if __name__ == "__main__":
    main()
