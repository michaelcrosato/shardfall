#!/usr/bin/env python3
"""Package the native studios, pinned creature compiler, portable Node, MCP server, and guides."""

import argparse
import hashlib
import json
from pathlib import Path
import zipfile

REPO = Path(__file__).resolve().parents[1]
FOLDER = "Shardfall-Studio"
QUICK_START = """# Shardfall Studio

## Start

1. Extract the whole ZIP to a writable folder.
2. Double-click Start-Creature-Studio.cmd. Keep the window open.
3. Add the server in mcp-live.example.json to your LLM client's MCP settings.
   Set command to the full path of the extracted pav.exe. Keep mcp and --live.
4. Reconnect the MCP server in your client, then give it a prompt.

The client must support local MCP servers. Shardfall does not need an API key.
The MCP server controls the open game at 127.0.0.1:7878.
The pinned SpawnForge compiler and portable Node runtime are included.
A saved ridgeback demo is ready to view without a first compile.

## First prompt

Copy the selected creature to moss_stalker. Make its horns shorter and its base
color dark teal. Keep its walk motion and my camera. Use creature_edit with named
paths and the current revision. Wait for the returned build job to publish.
Then confirm its frame ticket with studio_status and capture the result. Wait
for my next direction before making another change.

The Creatures panel has theme and template creation, blueprint editing, named
parts and surfaces, undo/redo, motion clips, and build status. Use right-drag to
orbit and the wheel to zoom. Fit frames the creature. F6 plays or pauses, F7
steps, and F8/F9 change speed.

The top bar switches Animations / Objects / Creatures / Game. Each workspace
keeps its selection, playback and camera. Accepted edits save automatically.
Failed and superseded builds keep the last valid creature visible.

Start-Object-Studio.cmd and Start-Animation-Studio.cmd open the other studios.
All launchers accept --backend dx12. The default renderer uses Vulkan.
To keep a level behind the stage, add --scene level/1.

## Saved work

Creatures: assets/creatures/workshop/name.json.
Creature build caches: assets/creatures/.compiled.
Creature undo history: assets/creatures/.editor.
Objects: assets/props/workshop/name.json; history: assets/props/.editor.
Animations: anim/workshop.json; history: anim/.editor.

Keep these folders when you replace the programs with a newer build.
Creature source blueprints are authoritative; caches can be rebuilt. Creature
Studio is an authoring and preview workspace. Spawning these rigs as gameplay
enemies and full SpawnForge shader materials are outside this release.
Object instances can be placed in scenes. Full snapshots keep exact definitions.
A normal hero save does not store a level layout or creature authoring history.

## Examples and reference

With the studio open, run from a command prompt in this folder:

    pav.exe live --stop-on-error < creature-ridgeback.jsonl
    pav.exe live --stop-on-error < object-lantern.jsonl
    pav.exe live --stop-on-error < animation-wave.jsonl

Each example uses its own asset name. The creature example queues an asynchronous
build; poll creature_status for its returned job before a dependent edit.

Read CREATURE_STUDIO.md, ASSET_STUDIO.md and ANIMATION_STUDIO.md for tool details.
BUILD.json records source, compiler pin, and executable hashes.
The programs target 64-bit Windows.
"""


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binaries", type=Path, default=REPO / "target/x86_64-pc-windows-gnu/dist")
    parser.add_argument("--out", type=Path, default=REPO / "out/shardfall-studio-windows.zip")
    parser.add_argument("--source", default="local build", help="verified source commit or ref")
    parser.add_argument("--node-runtime", type=Path, default=REPO / "tools/creature-compiler/runtime",
                        help="directory containing official win-x64 node.exe and LICENSE")
    parser.add_argument("--creature-demo", type=Path, required=True,
                        help="validated demo root containing workshop/*.json and .compiled/*.json")
    args = parser.parse_args()
    binaries = args.binaries.resolve()
    files = {
        "shardfall.exe": binaries / "shardfall.exe",
        "pav.exe": binaries / "pav.exe",
        "anim/moves.toml": REPO / "anim/moves.toml",
        "ASSET_STUDIO.md": REPO / "docs/ASSET_STUDIO.md",
        "CREATURE_STUDIO.md": REPO / "docs/CREATURE_STUDIO.md",
        "creature-ridgeback.jsonl": REPO / "scripts/examples/creature-ridgeback.jsonl",
        "runtime/node.exe": args.node_runtime / "node.exe",
        "runtime/NODE_LICENSE.txt": args.node_runtime / "LICENSE",
        "ANIMATION_STUDIO.md": REPO / "docs/ANIMATION_STUDIO.md",
        "object-lantern.jsonl": REPO / "scripts/examples/object-lantern.jsonl",
        "animation-wave.jsonl": REPO / "scripts/examples/animation-wave.jsonl",
    }
    for path in sorted((REPO / "assets/props/templates").glob("*.json")):
        files[str(path.relative_to(REPO))] = path
    compiler = REPO / "tools/creature-compiler"
    for name in ("worker.mjs", "dist/worker.mjs", "dist/THIRD_PARTY_NOTICES.txt", "pin.json",
                 "catalog.json", "blueprint.schema.json", "UPSTREAM.md"):
        files[f"tools/creature-compiler/{name}"] = compiler / name
    for path in sorted((compiler / "dist").glob("worker.mjs.LEGAL.txt")):
        files[f"tools/creature-compiler/dist/{path.name}"] = path
    for path in sorted((compiler / "templates").glob("*.json")):
        files[f"tools/creature-compiler/templates/{path.name}"] = path
    sources = sorted((args.creature_demo / "workshop").glob("*.json"))
    caches = sorted((args.creature_demo / ".compiled").glob("*.json"))
    if not sources or not caches:
        parser.error("The demo must contain both source and validated compiled cache files.")
    for path in sources + caches:
        files[f"assets/creatures/{path.relative_to(args.creature_demo)}"] = path
    for path in files.values():
        if not path.is_file():
            parser.error(f"Required file is missing: {path}")
    config = {"mcpServers": {"shardfall": {
        "command": "C:/path/to/Shardfall-Studio/pav.exe",
        "args": ["mcp", "--live"],
    }}}
    manifest = {"source": args.source, "target": "x86_64-pc-windows-gnu",
                "executables": {name: {"bytes": files[name].stat().st_size, "sha256": sha256(files[name])}
                                for name in ("shardfall.exe", "pav.exe", "runtime/node.exe")},
                "creature_compiler": {
                    "revision": json.loads((compiler / "pin.json").read_text())["revision"],
                    "adapter_format": 1,
                    "bundle_sha256": sha256(compiler / "dist/worker.mjs"),
                    "demo_sources": [path.name for path in sources],
                }}
    output = args.out.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
        for name, path in files.items():
            archive.write(path, f"{FOLDER}/{name}")
        archive.writestr(f"{FOLDER}/START_HERE.md", QUICK_START)
        archive.writestr(f"{FOLDER}/BUILD.json", json.dumps(manifest, indent=2) + "\n")
        archive.writestr(f"{FOLDER}/mcp-live.example.json", json.dumps(config, indent=2) + "\n")
        for label, flag in (("Object", "asset"), ("Animation", "animation"), ("Creature", "creature")):
            archive.writestr(f"{FOLDER}/Start-{label}-Studio.cmd",
                             f'@echo off\r\ncd /d "%~dp0"\r\nshardfall.exe --{flag}-studio %*\r\nif errorlevel 1 pause\r\n')
    print(json.dumps({"file": str(output), "bytes": output.stat().st_size, "sha256": sha256(output), **manifest}, indent=2))


if __name__ == "__main__":
    main()
