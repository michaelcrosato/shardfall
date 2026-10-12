#!/usr/bin/env python3
"""A review page the user flips through: cards of captures with one-key verdicts.

The visual checks of the asset plan (docs/ASSET_SYSTEM_PLAN.md §10.3: lean gates) go to the
user's eye, not to agent judges. This writes one HTML page (published as a private claude.ai
Artifact with the `db` capability) from a manifest:

    {"title": "Core Exemplars Review", "heading": "...", "intro": "...",
     "verdicts": ["ok", "fix", "drop"],            # optional, this is the default
     "items": [{"id": "barrel", "title": "LIB/core/barrel",
                "images": [{"path": "out/review/barrel.png", "label": "4 views"}],
                "facts": ["5 parts", "180 tok", "56x90x56 cm"],
                "note": "replaces the hard-coded crates"}]}

    python3 scripts/review_page.py manifest.json out/review/page.html

Images are embedded (data URIs; the page must stay under 16 MB). Verdicts and notes are kept in
the artifact's store, collection `verdicts`, one document per item id ({verdict, note}), which
the agent reads back with ArtifactData; without the store they stay in the browser, and
"Copy verdicts" gives them as text to paste into chat.
"""

import base64
import html
import json
import mimetypes
import sys
from pathlib import Path

PAGE = r"""<title>__TITLE__</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500&family=IBM+Plex+Sans+Condensed:wght@500;600&family=IBM+Plex+Sans:wght@400;500&display=swap">
<style>
/* Layout: a sticky tally bar over a wrapping grid of review cards, one capture per card. */
:root {
  --bg: #eef1f4; --surface: #ffffff; --line: #d5dbe2; --ink: #1d2733; --muted: #5b6878;
  --accent: #2f6fb0; --ok: #2e7d4f; --fix: #a8670f; --drop: #b3403a; --shade: rgba(29, 39, 51, .06);
  --display: "IBM Plex Sans Condensed", "Arial Narrow", system-ui, sans-serif;
  --body: "IBM Plex Sans", system-ui, -apple-system, "Segoe UI", sans-serif;
  --mono: "IBM Plex Mono", ui-monospace, "SFMono-Regular", Menlo, monospace;
}
@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) {
  --bg: #12161b; --surface: #1b2129; --line: #2c3540; --ink: #e6ebf1; --muted: #93a0b0;
  --accent: #6ea8e6; --ok: #5cbf86; --fix: #e0a24a; --drop: #e57a72; --shade: rgba(0, 0, 0, .25); color-scheme: dark } }
:root[data-theme="dark"] {
  --bg: #12161b; --surface: #1b2129; --line: #2c3540; --ink: #e6ebf1; --muted: #93a0b0;
  --accent: #6ea8e6; --ok: #5cbf86; --fix: #e0a24a; --drop: #e57a72; --shade: rgba(0, 0, 0, .25); color-scheme: dark }
body { background: var(--bg); color: var(--ink); font: 15px/1.5 var(--body); }
.wrap { max-width: 1280px; margin: 0 auto; padding-inline: 16px; padding-block: 20px 48px; display: grid; gap: 18px; }
header h1 { font: 600 1.7rem/1.15 var(--display); margin: 0; text-wrap: balance; letter-spacing: .01em; }
header p { margin: 6px 0 0; max-width: 68ch; color: var(--muted); }
.bar { position: sticky; top: env(safe-area-inset-top, 0px); z-index: 2; background: var(--bg); padding-block: 10px;
  display: flex; flex-wrap: wrap; gap: 8px 16px; align-items: center; border-bottom: 1px solid var(--line); }
.tally { display: flex; flex-wrap: wrap; gap: 6px; font: 500 .85rem var(--mono); font-variant-numeric: tabular-nums; }
.chip { padding: 2px 9px; border-radius: 99px; border: 1px solid var(--line); background: var(--surface); }
.chip b { font-weight: 500; }
.chip.ok b { color: var(--ok); } .chip.fix b { color: var(--fix); } .chip.drop b { color: var(--drop); }
.store { color: var(--muted); font-size: .85rem; flex: 1 1 220px; min-width: 0; }
button { font: 500 .9rem var(--body); color: var(--ink); background: var(--surface); border: 1px solid var(--line);
  border-radius: 6px; padding: 5px 12px; cursor: pointer; }
button:hover { border-color: var(--accent); }
button:focus-visible, textarea:focus-visible, .card:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.keys { color: var(--muted); font-size: .8rem; }
kbd { font: .78rem var(--mono); border: 1px solid var(--line); border-bottom-width: 2px; border-radius: 4px; padding: 0 4px; background: var(--surface); }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 380px), 1fr)); gap: 16px; }
.card { background: var(--surface); border: 1px solid var(--line); border-radius: 8px; display: grid; gap: 10px;
  padding: 12px; min-width: 0; box-shadow: 0 1px 2px var(--shade); }
.card[data-verdict="ok"] { border-color: var(--ok); } .card[data-verdict="fix"] { border-color: var(--fix); }
.card[data-verdict="drop"] { border-color: var(--drop); }
.card h2 { font: 500 .95rem var(--mono); margin: 0; overflow-wrap: anywhere; }
.facts { display: flex; flex-wrap: wrap; gap: 4px 10px; font: .8rem var(--mono); color: var(--muted); font-variant-numeric: tabular-nums; }
.note-in { color: var(--muted); font-size: .88rem; margin: 0; }
figure { margin: 0; display: grid; gap: 4px; }
figure img { width: 100%; height: auto; border-radius: 4px; background: var(--bg); cursor: zoom-in; }
figcaption { font-size: .78rem; color: var(--muted); }
.verdicts { display: flex; flex-wrap: wrap; gap: 6px; }
.verdicts button[aria-pressed="true"] { color: var(--surface); }
.verdicts button[data-v="ok"][aria-pressed="true"] { background: var(--ok); border-color: var(--ok); }
.verdicts button[data-v="fix"][aria-pressed="true"] { background: var(--fix); border-color: var(--fix); }
.verdicts button[data-v="drop"][aria-pressed="true"] { background: var(--drop); border-color: var(--drop); }
textarea { width: 100%; box-sizing: border-box; min-height: 2.6em; resize: vertical; font: .9rem var(--body);
  color: var(--ink); background: var(--bg); border: 1px solid var(--line); border-radius: 6px; padding: 6px 8px; }
.zoom { position: fixed; inset: 0; z-index: 5; background: var(--bg); display: grid; place-items: center; padding: 16px; cursor: zoom-out; }
.zoom img { max-width: 100%; max-height: 100%; object-fit: contain; }
.copied { color: var(--ok); font-size: .85rem; }
@media (prefers-reduced-motion: no-preference) { .card { transition: border-color .15s; } }
</style>
<div class="wrap">
  <header>
    <h1>__HEADING__</h1>
    <p>__INTRO__</p>
  </header>
  <div class="bar">
    <div class="tally" id="tally"></div>
    <div class="store" id="store">Loading saved verdicts…</div>
    <button type="button" id="copy">Copy verdicts</button>
    <span class="copied" id="copied" hidden>Copied</span>
  </div>
  <p class="keys">Click a card, then <kbd>1</kbd> __K1__ <kbd>2</kbd> __K2__ <kbd>3</kbd> __K3__, <kbd>j</kbd>/<kbd>k</kbd> next and previous. Click a picture to enlarge it.</p>
  <textarea id="copytext" hidden readonly aria-label="Verdicts as text"></textarea>
  <div class="grid" id="grid"></div>
</div>
<div class="zoom" id="zoom" hidden><img alt="" id="zoomimg"></div>
<script id="items" type="application/json">__ITEMS__</script>
<script>
(() => {
  const VERDICTS = __VERDICTS__;
  const items = JSON.parse(document.getElementById("items").textContent);
  const state = {};  // id -> {verdict, note}
  let db = null;
  const key = "review:" + document.title;
  const $ = (id) => document.getElementById(id);

  function loadLocal() {
    try { Object.assign(state, JSON.parse(localStorage.getItem(key) || "{}")); } catch (e) {}
  }
  function saveLocal() {
    try { localStorage.setItem(key, JSON.stringify(state)); } catch (e) {}
  }

  function card(item, index) {
    const el = document.createElement("article");
    el.className = "card"; el.tabIndex = 0; el.dataset.id = item.id; el.dataset.index = index;
    const h = document.createElement("h2"); h.textContent = item.title; el.append(h);
    if (item.facts && item.facts.length) {
      const f = document.createElement("div"); f.className = "facts";
      item.facts.forEach((t) => { const s = document.createElement("span"); s.textContent = t; f.append(s); });
      el.append(f);
    }
    if (item.note) { const n = document.createElement("p"); n.className = "note-in"; n.textContent = item.note; el.append(n); }
    (item.images || []).forEach((im) => {
      const fig = document.createElement("figure");
      const img = document.createElement("img"); img.src = im.src; img.alt = item.title + " " + (im.label || "");
      img.loading = "lazy"; img.addEventListener("click", () => { $("zoomimg").src = im.src; $("zoom").hidden = false; });
      fig.append(img);
      if (im.label) { const c = document.createElement("figcaption"); c.textContent = im.label; fig.append(c); }
      el.append(fig);
    });
    const row = document.createElement("div"); row.className = "verdicts";
    VERDICTS.forEach((v) => {
      const b = document.createElement("button"); b.type = "button"; b.dataset.v = v; b.textContent = v;
      b.setAttribute("aria-pressed", "false"); b.addEventListener("click", () => choose(item.id, v));
      row.append(b);
    });
    el.append(row);
    const t = document.createElement("textarea"); t.id = "note-" + item.id; t.placeholder = "What to change (optional)";
    t.setAttribute("aria-label", "Note on " + item.title);
    t.addEventListener("change", () => note(item.id, t.value));
    el.append(t);
    return el;
  }

  function paint() {
    const counts = Object.fromEntries(VERDICTS.map((v) => [v, 0]));
    let open = 0;
    items.forEach((item) => {
      const s = state[item.id] || {};
      const el = document.querySelector('.card[data-id="' + CSS.escape(item.id) + '"]');
      if (!el) return;
      el.dataset.verdict = s.verdict || "";
      el.querySelectorAll(".verdicts button").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.v === s.verdict)));
      const t = el.querySelector("textarea");
      if (document.activeElement !== t) t.value = s.note || "";
      if (s.verdict in counts) counts[s.verdict]++; else open++;
    });
    $("tally").innerHTML = "";
    VERDICTS.forEach((v) => {
      const c = document.createElement("span"); c.className = "chip " + v;
      c.innerHTML = "<b></b> "; c.querySelector("b").textContent = counts[v]; c.append(v); $("tally").append(c);
    });
    const c = document.createElement("span"); c.className = "chip"; c.textContent = open + " to review"; $("tally").append(c);
  }

  async function write(id) {
    const s = state[id] || {};
    if (db) {
      try { await db.collection("verdicts").doc(id).set({ verdict: s.verdict || "", note: s.note || "" }); return; }
      catch (e) { $("store").textContent = "Could not save to the shared store (" + (e && e.code || "error") + "); verdicts stay in this browser."; }
    }
    saveLocal();
  }
  function choose(id, v) {
    state[id] = Object.assign({}, state[id], { verdict: state[id] && state[id].verdict === v ? "" : v });
    paint(); write(id);
  }
  function note(id, text) {
    state[id] = Object.assign({}, state[id], { note: text.trim() });
    paint(); write(id);
  }

  function asText() {
    return items.map((item) => {
      const s = state[item.id] || {};
      return item.id + ": " + (s.verdict || "unreviewed") + (s.note ? " — " + s.note : "");
    }).join("\n");
  }
  $("copy").addEventListener("click", () => {
    const text = asText();
    const shown = () => { const t = $("copytext"); t.value = text; t.hidden = false; t.rows = Math.min(items.length, 16); t.select(); };
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(() => { $("copied").hidden = false; setTimeout(() => { $("copied").hidden = true; }, 1600); }, shown);
    } else shown();
  });
  $("zoom").addEventListener("click", () => { $("zoom").hidden = true; });

  document.addEventListener("keydown", (e) => {
    if (e.target.tagName === "TEXTAREA") return;
    if (e.key === "Escape") { $("zoom").hidden = true; return; }
    const cardEl = e.target.closest && e.target.closest(".card");
    const cards = [...document.querySelectorAll(".card")];
    if (e.key === "j" || e.key === "k") {
      const i = cardEl ? Number(cardEl.dataset.index) : -1;
      const next = cards[Math.max(0, Math.min(cards.length - 1, i + (e.key === "j" ? 1 : -1)))];
      if (next) { next.focus(); next.scrollIntoView({ block: "nearest" }); }
      return;
    }
    const n = Number(e.key);
    if (cardEl && n >= 1 && n <= VERDICTS.length) {
      choose(cardEl.dataset.id, VERDICTS[n - 1]);
      const next = cards[Number(cardEl.dataset.index) + 1];
      if (next) { next.focus(); next.scrollIntoView({ block: "nearest" }); }
    }
  });

  items.forEach((item, i) => $("grid").append(card(item, i)));
  loadLocal();
  paint();

  const use = window.claude && window.claude.use ? window.claude.use("db") : Promise.resolve(null);
  use.then((store) => {
    if (!store) {
      $("store").textContent = "Verdicts stay in this browser; use Copy verdicts to send them.";
      return;
    }
    db = store;
    $("store").textContent = "Verdicts save as you click and reach the agent directly.";
    db.collection("verdicts").onSnapshot((snap) => {
      snap.docs.forEach((d) => { if (d.exists) state[d.id] = Object.assign({}, d.data()); });
      paint();
    }, () => { $("store").textContent = "The shared store stopped answering; verdicts stay in this browser."; db = null; });
  }, () => { $("store").textContent = "Verdicts stay in this browser; use Copy verdicts to send them."; });
})();
</script>
"""


def data_uri(path: Path) -> str:
    kind = mimetypes.guess_type(path.name)[0] or "application/octet-stream"
    return f"data:{kind};base64,{base64.b64encode(path.read_bytes()).decode()}"


def main() -> None:
    if len(sys.argv) != 3:
        sys.exit("usage: review_page.py MANIFEST.json OUT.html")
    manifest_path = Path(sys.argv[1])
    manifest = json.loads(manifest_path.read_text())
    verdicts = manifest.get("verdicts", ["ok", "fix", "drop"])
    if not 1 <= len(verdicts) <= 3:
        sys.exit("verdicts: one to three labels")
    ids = [item["id"] for item in manifest["items"]]
    if len(set(ids)) != len(ids):
        sys.exit(f"item ids must be unique: {sorted({i for i in ids if ids.count(i) > 1})}")
    items = []
    for item in manifest["items"]:
        images = []
        for image in item.get("images", []):
            path = Path(image["path"])
            if not path.is_absolute():
                path = manifest_path.parent / path
            images.append({"src": data_uri(path), "label": image.get("label", "")})
        items.append({"id": item["id"], "title": item.get("title", item["id"]), "facts": item.get("facts", []),
                      "note": item.get("note", ""), "images": images})
    keys = [html.escape(v) for v in verdicts] + ["", ""]
    page = (PAGE.replace("__TITLE__", html.escape(manifest["title"]))
            .replace("__HEADING__", html.escape(manifest.get("heading", manifest["title"])))
            .replace("__INTRO__", html.escape(manifest.get("intro", "")))
            .replace("__K1__", keys[0]).replace("__K2__", keys[1]).replace("__K3__", keys[2])
            .replace("__VERDICTS__", json.dumps(verdicts))
            .replace("__ITEMS__", json.dumps(items).replace("</", "<\\/")))
    out = Path(sys.argv[2])
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(page)
    size = out.stat().st_size
    if size > 15_000_000:
        sys.exit(f"{out} is {size:,} bytes; the page limit is 16 MB (use smaller tiles)")
    print(f"{out}: {len(items)} items, {size:,} bytes")


if __name__ == "__main__":
    main()
