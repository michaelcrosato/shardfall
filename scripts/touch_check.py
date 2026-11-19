#!/usr/bin/env python3
"""Plays the browser build on an emulated phone with real touch events (CDP
`Input.dispatchTouchEvent`) and screenshots what each gesture does, held upright and sideways.
Headless Chromium, WebGPU on SwiftShader: a frame takes about a second, so each gesture's
events are sent together (as a phone delivers them, between two frames), not one round trip
apart, which would make a quick flick a slow drag.

usage: python3 -P scripts/touch_check.py PAGE.html OUT_DIR [--query Q]
       (-P, not -I: Playwright is a user package)

Upright (390x844), then sideways (844x390), in Emberwatch:
1. the HUD and the touch controls; the "Playing by touch" card swiped away (a notification);
2. two fingers at once: the left thumb pushes the stick up while the right one holds a skill
   (both show pressed: the stick's ring and the skill's button);
3. the bag button: the bag opens, fitted to the screen;
4. a quick sideways flick across the bag: it goes away;
5. the pause button: the pause menu opens; a flick across it closes it (menus swipe away too).
Then, sideways in the Proving Grounds: one tap to say it's a touch screen, and no more input
for a while: auto-attack (Settings, on by default on touch) fights the waves by itself; the
kills on the HUD show it. And again as Ysolde with Fireball as her first skill (a new save put
in the page): auto-attack then fires at the nearest foe in Fireball's range, the shooter's
"auto-fire at nearby enemies" (the first skill is what auto-attack uses).
The control rects come from `pav ui device=touch width=W height=H` (the same layout).
"""
import argparse
import asyncio
import json
import pathlib
import tomllib

from playwright.async_api import async_playwright

FLAGS = ["--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-vulkan=swiftshader", "--use-angle=swiftshader"]

# Where the controls sit in each orientation (centres, in CSS pixels), from the `ui` tool.
LAYOUTS = {
    "upright": {"size": (390, 844), "bag": (311, 33), "skill": (193, 723), "stick": (70, 640), "bag_body": (110, 560),
                "card": (150, 400), "menu": (357, 33), "menu_body": (110, 420)},
    "sideways": {"size": (844, 390), "bag": (765, 33), "skill": (647, 269), "stick": (110, 300), "bag_body": (560, 300),
                 "card": (380, 150), "menu": (811, 33), "menu_body": (330, 200)},
}


def event(kind, points):
    return {
        "type": kind,
        "touchPoints": [{"x": x, "y": y, "id": i, "radiusX": 8, "radiusY": 8, "force": 1} for i, (x, y) in points],
    }


async def gesture(cdp, *events):
    """Sends a gesture's events together: the game reads them in one frame."""
    await asyncio.gather(*(cdp.send("Input.dispatchTouchEvent", e) for e in events))


async def run(browser, page_url, out, name, lay):
    w, h = lay["size"]
    ctx = await browser.new_context(viewport={"width": w, "height": h}, is_mobile=True, has_touch=True, device_scale_factor=1)
    p = await ctx.new_page()
    errors = []
    p.on("pageerror", lambda e: errors.append(str(e)))
    await p.goto(page_url)
    cdp = await ctx.new_cdp_session(p)
    await p.wait_for_timeout(28000)
    await p.screenshot(path=str(out / f"{name}-1-hud.png"))
    # The "Playing by touch" card swiped away sideways (it would leave by itself only later).
    cx, cy = lay["card"]
    await gesture(
        cdp,
        event("touchStart", [(5, (cx, cy))]),
        event("touchMove", [(5, (cx + 90, cy + 3))]),
        event("touchMove", [(5, (cx + 200, cy + 5))]),
        event("touchEnd", []),
    )
    await p.wait_for_timeout(3500)
    await p.screenshot(path=str(out / f"{name}-1b-card-swiped.png"))
    # Two thumbs at once: the stick pushed up, a skill held.
    sx, sy = lay["stick"]
    await gesture(cdp, event("touchStart", [(1, (sx, sy)), (2, lay["skill"])]), event("touchMove", [(1, (sx, sy - 56)), (2, lay["skill"])]))
    await p.wait_for_timeout(4000)
    await p.screenshot(path=str(out / f"{name}-2-two-fingers.png"))
    await gesture(cdp, event("touchEnd", []))
    await p.wait_for_timeout(2500)
    await gesture(cdp, event("touchStart", [(9, lay["bag"])]), event("touchEnd", []))
    await p.wait_for_timeout(4000)
    await p.screenshot(path=str(out / f"{name}-3-bag.png"))
    # A quick flick sideways across the bag.
    bx, by = lay["bag_body"]
    await gesture(
        cdp,
        event("touchStart", [(3, (bx, by))]),
        event("touchMove", [(3, (bx + 80, by + 3))]),
        event("touchMove", [(3, (bx + 170, by + 6))]),
        event("touchEnd", []),
    )
    await p.wait_for_timeout(4000)
    await p.screenshot(path=str(out / f"{name}-4-flicked.png"))
    # The pause menu: its button, then a flick across it.
    await gesture(cdp, event("touchStart", [(11, lay["menu"])]), event("touchEnd", []))
    await p.wait_for_timeout(4000)
    await p.screenshot(path=str(out / f"{name}-5-menu.png"))
    mx, my = lay["menu_body"]
    await gesture(
        cdp,
        event("touchStart", [(12, (mx, my))]),
        event("touchMove", [(12, (mx + 90, my + 3))]),
        event("touchMove", [(12, (mx + 190, my + 6))]),
        event("touchEnd", []),
    )
    await p.wait_for_timeout(4000)
    await p.screenshot(path=str(out / f"{name}-6-menu-flicked.png"))
    print(name, "page errors:", errors[:3] if errors else "none")
    await ctx.close()


ROOT = pathlib.Path(__file__).resolve().parent.parent


def ranged_save():
    """A new Ysolde whose first skill is Fireball, as the game's save (localStorage
    `shardfall_save`, save.rs)."""
    heroes = tomllib.loads((ROOT / "game" / "heroes.toml").read_text())
    items = tomllib.loads((ROOT / "game" / "items.toml").read_text())
    c = heroes["ysolde"]
    bar = ["fireball"] + [k for k in c["bar"] if k != "fireball"]
    equipment = [None] * 10
    weapon = c.get("weapon")
    if weapon in items:
        equipment[0] = {"id": 1, "base": weapon, "level": 1, "rarity": "normal", "name": items[weapon].get("name", weapon)}
    hero = {"name": c.get("name", "Ysolde"), "character": "ysolde", "bar": bar, "equipment": equipment, "next_item": 2}
    return json.dumps({"version": 1, "hero": hero})


async def auto_attack(browser, page_url, out, name="arena", save=None):
    """Sideways in the Proving Grounds: one tap, then hands off while the waves come."""
    ctx = await browser.new_context(viewport={"width": 844, "height": 390}, is_mobile=True, has_touch=True, device_scale_factor=1)
    if save:
        await ctx.add_init_script("try { localStorage.setItem('shardfall_save', %s); } catch (e) {}" % json.dumps(save))
    p = await ctx.new_page()
    await p.goto(page_url)
    cdp = await ctx.new_cdp_session(p)
    await p.wait_for_timeout(25000)
    await gesture(cdp, event("touchStart", [(7, (420, 200))]), event("touchEnd", []))
    await p.wait_for_timeout(2000)
    await p.screenshot(path=str(out / f"{name}-1-start.png"))
    for k in range(5):
        await p.wait_for_timeout(10000)
        await p.screenshot(path=str(out / f"{name}-2-hands-off-{k}.png"))
    print(name, "auto-attack: screenshots taken")
    await ctx.close()


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("page")
    ap.add_argument("out")
    ap.add_argument("--query", default="?scene=town")
    ap.add_argument("--only-ranged", action="store_true", help="only the hands-off run with Fireball first")
    a = ap.parse_args()
    out = pathlib.Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    url = pathlib.Path(a.page).resolve().as_uri() + a.query
    async with async_playwright() as pw:
        browser = await pw.chromium.launch(args=FLAGS)
        for name, lay in ({} if a.only_ranged else LAYOUTS).items():
            await run(browser, url, out, name, lay)
        arena = pathlib.Path(a.page).resolve().as_uri() + "?scene=arena&seed=3"
        if not a.only_ranged:
            await auto_attack(browser, arena, out)
        await auto_attack(browser, arena, out, "ranged", ranged_save())
        await browser.close()


if __name__ == "__main__":
    asyncio.run(main())
