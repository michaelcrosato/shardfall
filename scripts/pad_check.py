#!/usr/bin/env python3
"""Plays the browser build with a virtual controller (any pad the browser reports in the standard
mapping: an Xbox pad by default, not a DualSense) and screenshots what it does: the page's
`navigator.getGamepads()` is replaced before the game starts, so the game's own input path
(gilrs in WebAssembly) reads it as it reads a real pad. Headless Chromium, WebGPU on SwiftShader:
a frame takes about a second, so presses are held long enough to be seen.

usage: python3 -P scripts/pad_check.py PAGE.html OUT_DIR [--query ?scene=town] [--id "NAME"]
       (-P, not -I: Playwright is a user package)

Steps: the game loads; Options opens the pause menu; D-pad down moves to the next item; Cross
takes it (Settings); each step is screenshotted (OUT_DIR/1-hud.png ... 4-settings.png).
"""
import argparse
import pathlib

from playwright.sync_api import sync_playwright

FLAGS = ["--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-vulkan=swiftshader", "--use-angle=swiftshader"]

PAD = """
(() => {
  const state = { id: __ID__, buttons: new Array(17).fill(0), axes: [0, 0, 0, 0] };
  window.__pad = state;
  const pad = () => ({
    id: state.id, index: 0, connected: true, mapping: 'standard', timestamp: performance.now(),
    axes: state.axes.slice(),
    buttons: state.buttons.map((v) => ({ pressed: v > 0.5, touched: v > 0, value: v })),
    vibrationActuator: { type: 'dual-rumble', playEffect: () => Promise.resolve('complete') },
  });
  Object.defineProperty(navigator, 'getGamepads', { value: () => [pad(), null, null, null] });
})();
"""


def press(p, button, ms=2500):
    p.evaluate(f"window.__pad.buttons[{button}] = 1")
    p.wait_for_timeout(ms)
    p.evaluate(f"window.__pad.buttons[{button}] = 0")
    p.wait_for_timeout(2500)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("page")
    ap.add_argument("out")
    ap.add_argument("--query", default="?scene=town")
    ap.add_argument("--id", default="Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)")
    a = ap.parse_args()
    out = pathlib.Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    with sync_playwright() as pw:
        browser = pw.chromium.launch(args=FLAGS)
        ctx = browser.new_context(viewport={"width": 1280, "height": 720})
        ctx.add_init_script(PAD.replace("__ID__", repr(a.id)))
        p = ctx.new_page()
        errors = []
        p.on("pageerror", lambda e: errors.append(str(e)))
        p.goto(pathlib.Path(a.page).resolve().as_uri() + a.query)
        p.wait_for_timeout(30000)
        # Any stick push makes the pad the device in use: the prompts turn to its buttons.
        p.evaluate("window.__pad.axes[0] = 0.6")
        p.wait_for_timeout(3000)
        p.evaluate("window.__pad.axes[0] = 0")
        p.wait_for_timeout(2500)
        p.screenshot(path=str(out / "1-hud.png"))
        press(p, 9)  # Options: the pause menu.
        p.screenshot(path=str(out / "2-menu.png"))
        press(p, 13)  # D-pad down: the next item.
        p.screenshot(path=str(out / "3-down.png"))
        press(p, 0)  # Cross: take it.
        p.wait_for_timeout(2500)
        p.screenshot(path=str(out / "4-settings.png"))
        print("page errors:", errors[:3] if errors else "none")
        browser.close()


if __name__ == "__main__":
    main()
