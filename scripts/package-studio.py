#!/usr/bin/env python3
"""Package the built Windows object and animation studio, MCP server, and guides."""

import argparse
import hashlib
import json
from pathlib import Path
import zipfile

REPO = Path(__file__).resolve().parents[1]
FOLDER = "Shardfall-Studio"
QUICK_START = """# Shardfall Studio

## Start

1. Extract this whole ZIP to a writable folder.
2. Double-click Start-Object-Studio.cmd. Keep the window open.
3. Add the server in mcp-live.example.json to your LLM client's MCP settings.
   Set command to the full path of the extracted pav.exe. Keep mcp and --live.
4. Reconnect the MCP server in your client, then give it a prompt.

The client must support local MCP servers. Shardfall does not need an API key.
The MCP server controls the open game at 127.0.0.1:7878.

## First prompt

Copy the bench template to garden_bench. Make its seat and backrest teal, use
metal legs, and add armrests. Apply related changes in one patch and show each
accepted edit in the live preview. Keep my camera in place. Use the last revision
for each edit. Capture the first version, then wait for my next direction.

The Objects panel has the same create, copy, part edit, undo, and placement tools
as the LLM. Use right-drag to orbit and the wheel to zoom. Fit frames the object.
F6 plays or pauses its turntable clock. The top bar switches Animations / Objects /
Game while keeping each workspace's selection and controls.

Start-Animation-Studio.cmd opens the animation workspace first. Both launchers
can select DirectX 12 with --backend dx12. The default renderer uses Vulkan.
To place props in a game scene, run Start-Object-Studio.cmd --scene level/1.

## Saved work

Objects save in assets/props/workshop/name.json. Undo history is in assets/props/.editor.
Animations save in anim/workshop.json; history is in anim/.editor.
Keep these folders when you replace the programs with a newer build.
Placed copies are in the current scene and full snapshots. A normal hero save
does not store a level layout. Room saves can retain object asset references.

## Try the examples

With the studio open, run these from a command prompt in this folder:

    pav.exe live --stop-on-error < object-lantern.jsonl
    pav.exe live --stop-on-error < animation-wave.jsonl

Each example creates a named asset and stops if that name already exists.

Read ASSET_STUDIO.md and ANIMATION_STUDIO.md for the full tool reference.
BUILD.json records a source reference and executable hashes. The programs target 64-bit Windows.
"""


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binaries", type=Path, default=REPO / "target/x86_64-pc-windows-gnu/dist")
    parser.add_argument("--out", type=Path, default=REPO / "out/shardfall-studio-windows.zip")
    parser.add_argument("--source", default="local build", help="verified source commit or ref")
    args = parser.parse_args()
    binaries = args.binaries.resolve()
    files = {
        "shardfall.exe": binaries / "shardfall.exe",
        "pav.exe": binaries / "pav.exe",
        "anim/moves.toml": REPO / "anim/moves.toml",
        "ASSET_STUDIO.md": REPO / "docs/ASSET_STUDIO.md",
        "ANIMATION_STUDIO.md": REPO / "docs/ANIMATION_STUDIO.md",
        "object-lantern.jsonl": REPO / "scripts/examples/object-lantern.jsonl",
        "animation-wave.jsonl": REPO / "scripts/examples/animation-wave.jsonl",
    }
    for path in sorted((REPO / "assets/props/templates").glob("*.json")):
        files[str(path.relative_to(REPO))] = path
    for path in files.values():
        if not path.is_file():
            parser.error(f"Required file is missing: {path}")
    config = {"mcpServers": {"shardfall": {
        "command": "C:/path/to/Shardfall-Studio/pav.exe",
        "args": ["mcp", "--live"],
    }}}
    manifest = {"source": args.source, "target": "x86_64-pc-windows-gnu",
                "executables": {name: {"bytes": files[name].stat().st_size, "sha256": sha256(files[name])}
                                for name in ("shardfall.exe", "pav.exe")}}
    output = args.out.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
        for name, path in files.items():
            archive.write(path, f"{FOLDER}/{name}")
        archive.writestr(f"{FOLDER}/START_HERE.md", QUICK_START)
        archive.writestr(f"{FOLDER}/BUILD.json", json.dumps(manifest, indent=2) + "\n")
        archive.writestr(f"{FOLDER}/mcp-live.example.json", json.dumps(config, indent=2) + "\n")
        for label, flag in (("Object", "asset"), ("Animation", "animation")):
            archive.writestr(f"{FOLDER}/Start-{label}-Studio.cmd",
                             f'@echo off\r\ncd /d "%~dp0"\r\nshardfall.exe --{flag}-studio %*\r\nif errorlevel 1 pause\r\n')
    print(json.dumps({"file": str(output), "bytes": output.stat().st_size, "sha256": sha256(output), **manifest}, indent=2))


if __name__ == "__main__":
    main()
