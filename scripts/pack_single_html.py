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
"""
import argparse
import base64
import gzip
import html
import json
import pathlib
import re
import sys

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
  msg.replaceChildren(`Loading ${document.title}…`, line);
  const paint = () => new Promise((resolve) => setTimeout(resolve, 20));
  // The JS glue is the module script at the end of the page: it hands over its init function.
  const glue = new Promise((resolve) => { window.gameGlue = resolve; });
  window.loadGame = async () => {
    line.textContent = 'Unpacking…';
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
      line.textContent = `Unpacking ${Math.floor((100 * n) / size)}%`;
    }
    if (n !== size) throw new Error(`the packed game is damaged (${n} of ${size} bytes)`);
    line.textContent = 'Starting…';
    await paint();
    const init = await glue;
    await init({ module_or_path: wasm });
  };
})();
"""


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
    page = replace_one(page, r"</body>", lambda m: tail + "</body>", "</body>")

    title = a.title if a.title is not None else html.unescape(re.search(r"<title>([^<]*)</title>", page)[1])
    out = a.out or ROOT / "target" / "web" / f"{re.sub(r'[^a-z0-9]+', '-', title.lower()).strip('-') or 'game'}.html"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(page, encoding="utf-8")
    mb = lambda n: f"{n / 1e6:.1f} MB"  # noqa: E731
    print(f"{out}: {mb(out.stat().st_size)} (wasm {mb(len(wasm))}, gzipped {mb(len(packed))}; JS glue {mb(len(glue))})")


if __name__ == "__main__":
    main()
