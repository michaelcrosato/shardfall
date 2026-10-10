#!/usr/bin/env python3
"""Package the built Windows Animation Studio and MCP server. Uses Python only."""

import argparse
import hashlib
import json
from pathlib import Path
import zipfile


REPO = Path(__file__).resolve().parents[1]
FOLDER = "Shardfall-Animation-Studio"

QUICK_START = """# Shardfall Animation Studio

## Start

1. Extract the whole ZIP to a writable folder.
2. Double-click Start-Animation-Studio.cmd. Keep the window open.
3. Add the server from mcp-live.example.json to your LLM client's MCP settings.
   Change the command path to the full path of the extracted pav.exe.
4. Reconnect the MCP server in your LLM client.

The LLM client must support local MCP servers. The game does not need an API key.
The MCP server connects to the open studio at 127.0.0.1:7878.
Use the `mcp --live` arguments shown in the example. Without --live, the server
uses a separate headless session and does not change the window.

## First prompt

Create a new two-second animation named Greeting. Add a right-handed wave.
Keep the feet still. Show each accepted edit in the live preview at half speed.
Inspect before each edit and use the returned revision. Capture a filmstrip
so you can check the motion.

## Controls and files

The Animation Studio panel can select, create, copy, play, pause, and edit clips.
Use right-drag to rotate the camera and the mouse wheel to zoom.
F6 plays or pauses. F7 steps one frame. F12 saves a window screenshot.

Accepted edits save automatically in anim/workshop.json. Each clip has 32
undo/redo steps, stored in anim/.editor. Copies of restricted local sources
stay in anim/local. Keep that folder local.

To run the supplied wave example, keep the studio open. Open a command prompt
in this folder and run:

    pav.exe live --stop-on-error < animation-wave.jsonl

The example creates WORKSHOP/HelloWave. It stops without editing it if that name exists.

Read ANIMATION_STUDIO.md for the full tool reference, setup, and file format.
This package contains the same engine and animation renderer as Shardfall.
The programs are built for 64-bit Windows. A Vulkan driver is the default;
use Start-Animation-Studio.cmd --backend dx12 to select DirectX 12.
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binaries", type=Path, default=REPO / "target/x86_64-pc-windows-gnu/dist")
    parser.add_argument("--out", type=Path, default=REPO / "out/shardfall-animation-studio-windows.zip")
    args = parser.parse_args()
    binaries = args.binaries.resolve()
    files = {
        "shardfall.exe": binaries / "shardfall.exe",
        "pav.exe": binaries / "pav.exe",
        "anim/moves.toml": REPO / "anim/moves.toml",
        "ANIMATION_STUDIO.md": REPO / "docs/ANIMATION_STUDIO.md",
        "animation-wave.jsonl": REPO / "scripts/examples/animation-wave.jsonl",
    }
    for path in files.values():
        if not path.is_file():
            parser.error(f"Required file is missing: {path}")
    config = {"mcpServers": {"shardfall": {
        "command": "C:/path/to/Shardfall-Animation-Studio/pav.exe",
        "args": ["mcp", "--live"],
    }}}
    output = args.out.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
        for name, path in files.items():
            archive.write(path, f"{FOLDER}/{name}")
        archive.writestr(f"{FOLDER}/START_HERE.md", QUICK_START)
        archive.writestr(f"{FOLDER}/mcp-live.example.json", json.dumps(config, indent=2) + "\n")
        archive.writestr(f"{FOLDER}/Start-Animation-Studio.cmd",
                         '@echo off\r\ncd /d "%~dp0"\r\nshardfall.exe --animation-studio %*\r\nif errorlevel 1 pause\r\n')
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    print(json.dumps({"file": str(output), "bytes": output.stat().st_size, "sha256": digest}, indent=2))


if __name__ == "__main__":
    main()
