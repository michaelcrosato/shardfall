#!/usr/bin/env python3
"""Packs the browser build into one self-contained HTML page: no other files, no network loads.

usage: python3 -I scripts/pack_single_html.py BINDGEN_DIR [--title T] [--description D]
           [--theme-color C] [--scene NAME] [--out PATH]

BINDGEN_DIR is wasm-bindgen's output (shardfall.js, shardfall_bg.wasm); scripts/build-web.sh
--single-file makes it and runs this. The page is web/index.html with the options written into
its <title>, <meta name=description> and <meta name=theme-color> (left as they are when not
given), the manifest link dropped, and, before </body>, the gzipped wasm as base64 text, a
loader that sets window.loadGame (index.html's script calls it when set) and the JS glue as an
inline module script. The loader shows a progress line, bakes in --scene (used when the address
names no room or scene) and unpacks with the browser's DecompressionStream. No blob: or data:
URLs are needed. Out: target/web/<title>.html.

--lines puts the game's line book (`pav lines out=`) in <head> as the game-lines block (AI Hub
Languages v1); --fonts DIR adds each .ttf there as a font-NAME block (zlib, base64) that the
game decodes only when its language needs it. --hub web/hub.json writes the AI Hub Catalogue v1
and Session Protocol v1 tags into <head> after pasting the session client (web/hub/session.js)
as the page's first script; --session-mode picks parallel, ghost or shared-world (default: the
file's), --model and --effort fill hub:model and hub:effort (docs/AI_HUB.md). Every page gets
the emblem as its icon.
"""
import argparse
import base64
import gzip
import html
import json
import pathlib
import re
import sys
import urllib.parse
import zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
PAGE = ROOT / "web" / "index.html"

# Runs while the page parses, before index.html's module script.
LOADER = """
(() => {
  const msg = document.getElementById('msg');
  // The start scene baked in at build time, unless the address names a room or a scene.
  const scene = __SCENE__, query = new URLSearchParams(location.search);
  if (scene && !query.get('room') && !query.get('scene')) {
    query.set('scene', scene);
    try { history.replaceState(history.state, '', '?' + query + location.hash); }
    catch (error) { console.warn('Could not put the start scene in the address:', error); }
  }
  const line = document.createElement('div');
  line.style.cssText = 'font-size: 14px; opacity: 0.7; font-variant-numeric: tabular-nums';
  msg.style.flexDirection = 'column';
  msg.style.gap = '8px';
  const name = document.title.split(' — ')[0];
  msg.replaceChildren(pageLine('page.loading', 'Loading {name}…', { name }), line);
  const paint = () => new Promise((resolve) => setTimeout(resolve, 20));
  // The JS glue is the module script at the end of the page: it hands over its init function.
  const glue = new Promise((resolve) => { window.gameGlue = resolve; });
  window.loadGame = async () => {
    line.textContent = pageLine('page.unpacking', 'Unpacking…');
    await paint();
    // The gzipped wasm's base64 block leaves the page once read, to free its text.
    const block = document.getElementById('game-wasm'), size = +block.dataset.size, text = block.textContent;
    block.remove();
    let packed;
    if (Uint8Array.fromBase64) packed = Uint8Array.fromBase64(text);
    else {
      const bin = atob(text);
      packed = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) packed[i] = bin.charCodeAt(i);
    }
    const wasm = new Uint8Array(size);
    const reader = new Blob([packed]).stream().pipeThrough(new DecompressionStream('gzip')).getReader();
    let n = 0;
    for (let r = await reader.read(); !r.done; r = await reader.read()) {
      wasm.set(r.value, n);
      n += r.value.length;
      line.textContent = pageLine('page.unpacking_percent', 'Unpacking {percent}%', { percent: Math.floor((100 * n) / size) });
    }
    if (n !== size) throw new Error(`the packed game is damaged (${n} of ${size} bytes)`);
    line.textContent = pageLine('page.starting', 'Starting…');
    await paint();
    const init = await glue;
    await init({ module_or_path: wasm });
  };
})();
"""


# The emblem (three falling shards, as on the title screen), a square SVG for <link rel=icon>.
EMBLEM = (
    "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'>"
    "<rect width='64' height='64' rx='14' fill='#15171c'/>"
    "<g stroke='#dcf5ff' stroke-width='1.2' stroke-linejoin='round'>"
    "<path d='M32 6 L40 30 L32 58 Z' fill='#466ebe'/><path d='M32 6 L32 58 L24 30 Z' fill='#96dcff'/>"
    "<path d='M15 20 L20 33 L16 46 Z' fill='#466ebe' transform='rotate(-18 17 33)'/>"
    "<path d='M15 20 L16 46 L11 33 Z' fill='#96dcff' transform='rotate(-18 17 33)'/>"
    "<path d='M49 22 L54 35 L49 48 Z' fill='#466ebe' transform='rotate(20 49 35)'/>"
    "<path d='M49 22 L49 48 L44 35 Z' fill='#96dcff' transform='rotate(20 49 35)'/>"
    "</g></svg>"
)
ICON = "data:image/svg+xml," + urllib.parse.quote(EMBLEM, safe=" =:/'#")

HUB_LIMITS = {"name": 40, "tagline": 60, "description": 200, "controls": 160, "goal": 60, "howto": 80, "model": 40, "harness": 40}
EFFORTS = ("low", "medium", "high", "xhigh", "max", "ultra")


def hub_head(cfg, mode, model, effort):
    """The catalogue's and the session protocol's tags, in the catalogue's order."""
    if mode not in ("parallel", "ghost", "shared-world"):
        fail(f"--session-mode {mode}: parallel, ghost or shared-world")
    tags = [t for t in cfg["tags"] if t not in ("co-op", "versus", "race")]
    together = {"ghost": "race", "shared-world": "co-op"}.get(mode)
    if together:
        tags.append(together)
    goal = cfg["goals"][mode]
    checks = {"name": cfg["name"], "tagline": cfg["tagline"], "description": cfg["description"], "controls": cfg["controls"],
              "goal": goal, "howto": cfg["session_howto"], "model": model or "", "harness": cfg["harness"]}
    for key, value in checks.items():
        if len(value) > HUB_LIMITS[key]:
            fail(f"hub {key} is {len(value)} characters: at most {HUB_LIMITS[key]}")
    if not model:
        fail("--hub needs --model: the model that wrote this build, as its maker names it")
    if effort and effort not in EFFORTS:
        fail(f"--effort {effort}: one of {', '.join(EFFORTS)}")
    if not 2 <= len(tags) <= 5:
        fail(f"hub tags: 2 to 5, not {len(tags)}")
    meta = lambda name, value: f'<meta name="{name}" content="{html.escape(str(value))}">'  # noqa: E731
    lines = [
        meta("hub:catalogue", 1),
        meta("hub:type", cfg["type"]),
        meta("hub:genre", cfg["genre"]),
        meta("hub:tags", ", ".join(tags)),
        meta("hub:controls", cfg["controls"]),
        meta("hub:model", model),
    ]
    if effort:
        lines.append(meta("hub:effort", effort))
    lines += [meta("hub:harness", cfg["harness"]), meta("hub:session", 1)]
    if together:
        lines.append(meta("hub:players", cfg["session_players"]))
    if cfg.get("webgpu"):
        lines.append(meta("hub:webgpu", cfg["webgpu"]))
    lines += [
        meta("hub:session-players", cfg["session_players"]),
        meta("hub:session-mode", mode),
        meta("hub:session-goal", goal),
        meta("hub:session-howto", cfg["session_howto"]),
        meta("hub:session-minutes", cfg["session_minutes"]),
    ]
    return "\n".join(lines) + "\n"


def fail(text):
    sys.exit(f"pack_single_html: {text}")


def replace_one(page, pattern, make, what):
    """Replaces the one match of `pattern` in the page; anything else means index.html changed."""
    page, n = re.subn(pattern, make, page)
    if n != 1:
        fail(f"expected one {what} in {PAGE.relative_to(ROOT)}, found {n}: update this script")
    return page


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("bindgen_dir", type=pathlib.Path, help="wasm-bindgen output (shardfall.js, shardfall_bg.wasm)")
    ap.add_argument("--title", help="<title> (default: index.html's)")
    ap.add_argument("--description", help="<meta name=description> (default: index.html's)")
    ap.add_argument("--theme-color", help="<meta name=theme-color> (default: index.html's)")
    ap.add_argument("--scene", help="start scene when the address names no room or scene (default: the game's)")
    ap.add_argument("--out", type=pathlib.Path, help="output file (default: target/web/<title>.html)")
    ap.add_argument("--lines", type=pathlib.Path, help="the game's line book (JSON, `pav lines out=`): the game-lines block")
    ap.add_argument("--fonts", type=pathlib.Path, help="folder of .ttf fonts to carry (assets/fonts)")
    ap.add_argument("--hub", type=pathlib.Path, help="the AI Hub's tags (web/hub.json): catalogue, session client and session tags")
    ap.add_argument("--session-mode", help="parallel, ghost or shared-world (default: --hub file's)")
    ap.add_argument("--model", help="hub:model: the model that wrote this build, as its maker names it")
    ap.add_argument("--effort", help="hub:effort: the effort it ran at")
    a = ap.parse_args()

    js, wasm_path = a.bindgen_dir / "shardfall.js", a.bindgen_dir / "shardfall_bg.wasm"
    for p in (js, wasm_path):
        if not p.is_file():
            fail(f"{p} is missing: run wasm-bindgen --target web --out-name shardfall first")
    # The glue goes in the page as an inline module script, so it can't import other files
    # (wasm-bindgen snippets) or hold text that would end the script element early.
    glue = js.read_text(encoding="utf-8")
    if (a.bindgen_dir / "snippets").exists() or re.search(r"^import\b", glue, re.M):
        fail("the JS glue imports other files (wasm-bindgen snippets); inline them before packing")
    if re.search(r"(?i)</script|<script|<!--", glue):
        fail("the JS glue contains <script, </script or <!--, which can't sit inside an inline script")
    init = re.search(r"^export \{[^}]*?\b([\w$]+) as default\b", glue, re.M)
    if not init:
        fail("can't find the JS glue's default export (its init function)")

    page = PAGE.read_text(encoding="utf-8")
    if a.title is not None:
        page = replace_one(page, r"<title>[^<]*</title>", lambda m: f"<title>{html.escape(a.title)}</title>", "<title>")
    for name, value in (("description", a.description), ("theme-color", a.theme_color)):
        if value is not None:
            page = replace_one(
                page,
                rf'<meta name="{name}" content="[^"]*">',
                lambda m, name=name, value=value: f'<meta name="{name}" content="{html.escape(value)}">',
                f"<meta name={name}>",
            )
    # A manifest is another file to load: the single page goes without one.
    page = replace_one(page, r'[ \t]*<link rel="manifest"[^>]*>\n', lambda m: "", "manifest link")
    # The emblem is the icon; after it come the hub's tags and the line book, before any script.
    after_icon = ""
    if a.hub:
        cfg = json.loads(a.hub.read_text(encoding="utf-8"))
        mode = a.session_mode or cfg["session_mode"]
        page = replace_one(page, r"<title>[^<]*</title>", lambda m: f"<title>{html.escape(cfg['name'] + ' — ' + cfg['tagline'])}</title>", "<title>")
        for name, value in (("description", cfg["description"]), ("theme-color", cfg["theme_color"])):
            page = replace_one(
                page,
                rf'<meta name="{name}" content="[^"]*">',
                lambda m, name=name, value=value: f'<meta name="{name}" content="{html.escape(value)}">',
                f"<meta name={name}>",
            )
        after_icon += hub_head(cfg, mode, a.model, a.effort)
        client = (ROOT / "web" / "hub" / "session.js").read_text(encoding="utf-8")
        if "</script" in client.lower():
            fail("web/hub/session.js contains </script")
        # The session client, unmodified, is the page's first script, right after <meta charset>.
        page = replace_one(page, r'<meta charset="utf-8">\n', lambda m: m[0] + f"<script>{client}</script>\n", "<meta charset>")
    if a.lines:
        book = json.loads(a.lines.read_text(encoding="utf-8"))
        block = json.dumps(book, ensure_ascii=False, indent=1).replace("<", "\\u003c")
        page = replace_one(page, r'<html lang="[^"]*">', lambda m: f'<html lang="{html.escape(book["language"])}">', "<html lang>")
        after_icon += f'<script type="application/json" id="game-lines">\n{block}\n</script>\n'
    page = replace_one(page, r'<link rel="icon" href="[^"]*">\n', lambda m: f'<link rel="icon" href="{ICON}">\n' + after_icon, "icon link")

    wasm = wasm_path.read_bytes()
    packed = gzip.compress(wasm, compresslevel=9, mtime=0)
    # JSON is valid JS; escaping "<" keeps a scene name from closing the script element.
    loader = LOADER.replace("__SCENE__", json.dumps(a.scene or None).replace("<", "\\u003c"))
    tail = (
        f'<script id="game-wasm" type="application/octet-stream" data-size="{len(wasm)}">'
        f"{base64.b64encode(packed).decode()}</script>\n"
        f"<script>{loader}</script>\n"
        f'<script type="module">\n{glue}\nwindow.gameGlue({init[1]});\n</script>\n'
    )
    if a.fonts:
        for font in sorted(a.fonts.glob("*.ttf")):
            data = base64.b64encode(zlib.compress(font.read_bytes(), 9)).decode()
            tail += f'<script id="font-{font.stem}" type="application/octet-stream">{data}</script>\n'
    page = replace_one(page, r"</body>", lambda m: tail + "</body>", "</body>")

    title = a.title if a.title is not None else html.unescape(re.search(r"<title>([^<]*)</title>", page)[1])
    out = a.out or ROOT / "target" / "web" / f"{re.sub(r'[^a-z0-9]+', '-', title.lower()).strip('-') or 'game'}.html"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(page, encoding="utf-8")
    mb = lambda n: f"{n / 1e6:.1f} MB"  # noqa: E731
    print(f"{out}: {mb(out.stat().st_size)} (wasm {mb(len(wasm))}, gzipped {mb(len(packed))}; JS glue {mb(len(glue))})")


if __name__ == "__main__":
    main()
