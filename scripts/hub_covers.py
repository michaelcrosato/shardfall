#!/usr/bin/env python3
"""52in1's three screenshots of Shardfall (AI Hub Catalogue v1, docs/AI_HUB.md): the finished page
in headless Chromium (WebGPU on SwiftShader), outside a session, in its own language, with nothing
over it that isn't the game's. A 960 x 720 window at a device scale factor of 2/3, JPEG quality 58,
gives the hub's 640 x 480 baseline frames.

usage: python3 -P scripts/hub_covers.py PAGE.html OUT_DIR   (-P, not -I: Playwright is a user package)

0.jpg: the opening view, the Pavilion's plaza. 1.jpg, 2.jpg: play (the Proving Grounds' waves, a
descent level). Software rendering is slow (a frame takes about a second), so each moment is given
time.
"""
import pathlib
import sys

from playwright.sync_api import sync_playwright

FLAGS = ["--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-vulkan=swiftshader", "--use-angle=swiftshader"]


def shot(browser, url, query, out, act):
    ctx = browser.new_context(viewport={"width": 960, "height": 720}, device_scale_factor=2 / 3)
    p = ctx.new_page()
    errors = []
    p.on("pageerror", lambda e: errors.append(str(e)))
    p.on("console", lambda m: errors.append(m.text) if m.type == "error" else None)
    p.goto(url + query)
    p.wait_for_timeout(25000)
    act(p)
    p.screenshot(path=str(out), type="jpeg", quality=58, timeout=180000)
    print(f"{out.name}{' (console: ' + '; '.join(errors[:3]) + ')' if errors else ''}", flush=True)
    ctx.close()


def hold(p, code, ms):
    p.keyboard.down(code)
    p.wait_for_timeout(ms)
    p.keyboard.up(code)


def arena(p):
    # The Proving Grounds: a wave closes in and the hero swings and casts.
    p.mouse.move(600, 300)
    p.wait_for_timeout(20000)
    for k in range(6):
        p.mouse.down()
        p.wait_for_timeout(1500)
        p.mouse.up()
        hold(p, "KeyR" if k % 2 else "KeyQ", 600)
    p.wait_for_timeout(4000)


def descent(p):
    # A descent level: down the first corridor toward the first room.
    p.mouse.move(480, 200)
    hold(p, "KeyW", 14000)
    p.mouse.down()
    p.wait_for_timeout(2500)
    p.mouse.up()
    p.wait_for_timeout(3000)


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    page, out = pathlib.Path(sys.argv[1]).resolve(), pathlib.Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    url = page.as_uri()
    with sync_playwright() as pw:
        browser = pw.chromium.launch(args=FLAGS)
        # The opening view: the splash over, the camera down from over the plaza to the player, with
        # the welcome and the studios' and tools' names. (No click: on the plaza it throws a bomb.)
        shot(browser, url, "", out / "0.jpg", lambda p: p.wait_for_timeout(25000))
        shot(browser, url, "?scene=arena&seed=3", out / "1.jpg", arena)
        shot(browser, url, "?scene=level/6&seed=5", out / "2.jpg", descent)
        browser.close()


if __name__ == "__main__":
    main()
