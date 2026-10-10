#!/usr/bin/env python3
"""Check all bundled templates through native MCP, without a window or capture.

Build pav_tools and the pinned Node bundle first. Each template is built at low
quality in sequence. The native parser must accept it, then its rest pose and
one baked clip frame must match a separate matrix calculation. Source files and
caches use a temporary folder; only a compact JSON report remains.
"""

import argparse
from datetime import datetime, timezone
import importlib.util
import json
import math
import os
from pathlib import Path
import sys
import tempfile
import time


REPO = Path(__file__).resolve().parents[1]
PIN = "851880256987ecdb2895c6afd01f84df64199bdb"
TOLERANCE = 0.0005
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("animation_smoke", REPO / "scripts/smoke-animation-studio.py")
helpers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helpers)


def transform(position, rotation):
    """A row-major rigid matrix from a position and an xyzw quaternion."""
    size = math.sqrt(sum(v * v for v in rotation))
    x, y, z, w = [v / size for v in rotation]
    return [
        [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w), position[0]],
        [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w), position[1]],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y), position[2]],
        [0, 0, 0, 1],
    ]


def multiply(a, b):
    return [[sum(a[row][k] * b[k][col] for k in range(4)) for col in range(4)] for row in range(4)]


def inverse_rigid(matrix):
    rotation = [[matrix[col][row] for col in range(3)] for row in range(3)]
    return [rotation[row] + [-sum(rotation[row][k] * matrix[k][3] for k in range(3))]
            for row in range(3)] + [[0, 0, 0, 1]]


def matrices(positions, rotations, count, frame=0):
    return [transform(positions[i * 3:i * 3 + 3], rotations[i * 4:i * 4 + 4])
            for i in range(frame * count, (frame + 1) * count)]


def world_pose(asset, clip=None, frame=0):
    """Apply the wire contract with rigid matrices, independently of native quaternion FK."""
    bones = asset["bones"]
    count = len(bones["names"])
    if clip is not None:
        track = asset["clips"][clip]
        local = matrices(track["positions"], track["rotations"], count, frame)
    else:
        bind = matrices(bones["positions"], bones["rotations"], count)
        if not bones.get("rest"):
            return bind
        local = []
        for i, parent in enumerate(bones["parents"]):
            delta = multiply(inverse_rigid(bind[parent]), bind[i]) if parent >= 0 else bind[i]
            local.append(transform([delta[row][3] for row in range(3)], bones["rest"][i * 4:i * 4 + 4]))
    world = []
    for i, parent in enumerate(bones["parents"]):
        world.append(multiply(world[parent], local[i]) if parent >= 0 else local[i])
    return world


def compare_pose(asset, actual, expected):
    bones = asset["bones"]
    assert len(actual["joints"]) == len(bones["names"]), "joint count changed"
    lift = [0, -asset["bounds"]["min"][1], 0]
    position_error = rotation_error = 0.0
    for i, joint in enumerate(actual["joints"]):
        assert (joint["name"], joint["parent"]) == (bones["names"][i], bones["parents"][i]), joint
        assert all(math.isfinite(v) for v in joint["position"] + joint["rotation"]), joint
        assert abs(sum(v * v for v in joint["rotation"]) - 1) < TOLERANCE, joint
        matrix = transform(joint["position"], joint["rotation"])
        dp = max(abs(matrix[row][3] - expected[i][row][3] - lift[row]) for row in range(3))
        dr = max(abs(matrix[row][col] - expected[i][row][col]) for row in range(3) for col in range(3))
        assert dp < TOLERANCE and dr < TOLERANCE, f"{joint['name']}: position error {dp}, rotation error {dr}"
        position_error, rotation_error = max(position_error, dp), max(rotation_error, dr)
    return {"max_position_error_m": position_error, "max_rotation_matrix_error": rotation_error}


def wait_job(client, queued):
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        status = client.call("creature_status", {"job": queued["job"]})
        if status["state"] in ("published", "failed", "superseded"):
            assert status["state"] == "published", status
            assert "preview_error" not in status, status
            return status
        time.sleep(0.1)
    raise AssertionError(f"Job {queued['job']} did not finish within 120 seconds")


def check_template(client, root, template):
    leaf = "native_" + template
    saved = wait_job(client, client.call("creature_edit", {
        "action": "create", "name": leaf, "template": template, "quality": "low",
        "if_revision": "absent", "preview": True,
    }))
    asset = json.loads((root / ".compiled" / f"{leaf}--{saved['revision']}.json").read_text())
    assert asset["format"] == 1 and asset["generator_revision"] == PIN, "unexpected native format or compiler pin"
    assert asset["source_revision"] == saved["revision"] and asset["name"] == leaf, "wrong native cache"

    rest = client.call("creature_preview", {
        "action": "pose", "name": saved["name"], "clip": "rest", "time": 0,
        "playing": False, "turntable": False, "yaw": 0, "scale": 1,
    })
    assert rest["revision"] == saved["asset_revision"] and rest["source_revision"] == saved["revision"], rest
    rest_error = compare_pose(asset, rest, world_pose(asset))

    clips = asset["clips"]
    preferred = ("fly", "swim.undulate", "tripod", "gallop", "walk")
    clip = next((name for name in preferred if name in clips), next(iter(clips)))
    track = clips[clip]
    frame = (track["frames"] - 1) // 2
    when = track["duration"] * frame / (track["frames"] - 1)
    animated = client.call("creature_preview", {"action": "pose", "clip": clip, "time": when, "playing": False})
    assert math.isclose(animated["time"], when, abs_tol=0.000001), "the requested frame was not selected"
    clip_error = compare_pose(asset, animated, world_pose(asset, clip, frame))
    return {
        "template": template, "ok": True, "native_parse": "accepted",
        "source_revision": saved["revision"], "asset_revision": saved["asset_revision"],
        "bones": rest["bones"], "vertices": rest["vertices"], "triangles": rest["triangles"],
        "has_local_rest": bool(asset["bones"].get("rest")), "rest": rest_error,
        "clip": {"name": clip, "frame": frame, "time": animated["time"], **clip_error},
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pav", type=Path, default=REPO / "target/debug/pav")
    parser.add_argument("--out", type=Path, default=REPO / "out/creature-studio-validation/native-templates.json")
    args = parser.parse_args()
    binary = args.pav.resolve()
    if not binary.is_file():
        parser.error(f"Build pav_tools first: {binary} is missing")
    templates = sorted(path.stem for path in (REPO / "tools/creature-compiler/templates").glob("*.json"))
    assert len(templates) == 8, f"expected 8 bundled templates, got {templates}"
    results = []
    with tempfile.TemporaryDirectory(prefix="shardfall-native-creatures-") as temporary:
        root = Path(temporary)
        env = {**os.environ, "PAV_CREATURES": str(root), "PAV_ANIM": str(root / "anim"), "PAV_ASSETS": str(root / "props")}
        client = helpers.Mcp(binary, env)
        try:
            for template in templates:
                try:
                    result = check_template(client, root, template)
                except Exception as error:
                    result = {"template": template, "ok": False, "error": str(error)[:2000]}
                results.append(result)
                print(("PASS: " if result["ok"] else "FAIL: ") + template, flush=True)
        finally:
            client.close()
    report = {
        "checked_at_utc": datetime.now(timezone.utc).isoformat(), "binary": str(binary),
        "generator_revision": PIN, "adapter_format": 1, "quality": "low",
        "scope": "Native parse and stage-space rest/one clip pose; CPU only, sequential, no captures",
        "tolerance": TOLERANCE, "templates": results,
        "passed": sum(result["ok"] for result in results), "failed": sum(not result["ok"] for result in results),
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(f"{report['passed']}/8 passed; {args.out}")
    return 1 if report["failed"] else 0


if __name__ == "__main__":
    sys.exit(main())
