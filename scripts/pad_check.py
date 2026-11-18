#!/usr/bin/env python3
"""Plays the browser build with a virtual controller and screenshots what it does: the page's
`navigator.getGamepads()` is replaced before the game starts, so the game's own input path
(gilrs in WebAssembly, behind the page's reading of unmapped pads) reads it as it reads a real
pad. Headless Chromium, WebGPU on SwiftShader: a frame takes about a second, so presses are held
long enough to be seen.

usage: python3 -P scripts/pad_check.py PAGE.html OUT_DIR [--unmapped] [--dualsense] [--query Q]
       (-P, not -I: Playwright is a user package)

- Default: an Xbox pad in the standard mapping. Options opens the pause menu, D-pad down moves
  to the next item, Cross takes it (Settings); each step is screenshotted.
- --unmapped: a generic pad the browser does not map (mapping '', 12 buttons, the D-pad a hat
  switch on axis 9), the same steps: the page reads it in the standard layout.
- --dualsense: a DualSense, with 52in1's player settings (window.AIHubSettings, its DualSense's
  extras allowed) put in the page as the hub's companion would: in the Proving Grounds the hero
  is hit, and the light bar's colours, the triggers' effects, the rumbles and any request for
  the microphone are recorded (OUT_DIR/dualsense.json); the touchpad's press opens the map.
"""
import argparse
import json
import pathlib

from playwright.sync_api import sync_playwright

FLAGS = ["--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-vulkan=swiftshader", "--use-angle=swiftshader"]

PAD = """
(() => {
  const unmapped = __UNMAPPED__;
  const state = { id: __ID__, buttons: new Array(unmapped ? 12 : 18).fill(0),
                  axes: unmapped ? [0, 0, 0, 0, 0, 0, 0, 0, 0, 1.2857] : [0, 0, 0, 0] };
  window.__pad = state;
  window.__calls = { rumble: [], light: [], trigger: [], mic: 0 };
  const pad = () => ({
    id: state.id, index: 0, connected: true, mapping: unmapped ? '' : 'standard', timestamp: performance.now(),
    axes: state.axes.slice(),
    buttons: state.buttons.map((v) => ({ pressed: v > 0.5, touched: v > 0, value: v })),
    vibrationActuator: { type: 'dual-rumble', playEffect: (kind, p) => { window.__calls.rumble.push(p); return Promise.resolve('complete'); } },
  });
  Object.defineProperty(navigator, 'getGamepads', { configurable: true, value: () => [pad(), null, null, null] });
  if (navigator.mediaDevices) {
    navigator.mediaDevices.getUserMedia = () => { window.__calls.mic++; return Promise.reject(new Error('no')); };
  }
  if (__DUALSENSE__) {
    // 52in1's player settings, as its companion script gives every game page: a DualSense with
    // its extras allowed, the microphone not.
    window.AIHubSettings = {
      music: 1, effects: 1, vibration: 1, mic: false, micLater: false, muted: false,
      dualsense: {
        connected: true, lightbar: true, triggers: true, motion: true, touchpad: true, speaker: false, aim: false,
        gyro: { x: 0, y: 0, z: 0 }, accel: { x: 0, y: 1, z: 0 }, touches: [],
        light: (c) => window.__calls.light.push(c),
        trigger: (w, e) => window.__calls.trigger.push([w, e]),
      },
      addEventListener: () => {},
    };
  }
})();
"""


def press(p, button, ms=2500):
    p.evaluate(f"window.__pad.buttons[{button}] = 1")
    p.wait_for_timeout(ms)
    p.evaluate(f"window.__pad.buttons[{button}] = 0")
    p.wait_for_timeout(2500)


def dpad_down(p, unmapped):
    if not unmapped:
        return press(p, 13)
    # A hat switch: down is 1/7 on its axis, centred past 1.
    p.evaluate("window.__pad.axes[9] = 1 / 7")
    p.wait_for_timeout(2500)
    p.evaluate("window.__pad.axes[9] = 1.2857")
    p.wait_for_timeout(2500)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("page")
    ap.add_argument("out")
    ap.add_argument("--query", default=None)
    ap.add_argument("--unmapped", action="store_true")
    ap.add_argument("--dualsense", action="store_true")
    a = ap.parse_args()
    out = pathlib.Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    pad_id = (
        "DualSense Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)" if a.dualsense
        else "USB Gamepad (Vendor: 0079 Product: 0006)" if a.unmapped
        else "Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)"
    )
    query = a.query or ("?scene=arena&seed=3" if a.dualsense else "?scene=town")
    script = PAD.replace("__ID__", repr(pad_id)).replace("__UNMAPPED__", "true" if a.unmapped else "false")
    script = script.replace("__DUALSENSE__", "true" if a.dualsense else "false")
    with sync_playwright() as pw:
        browser = pw.chromium.launch(args=FLAGS)
        ctx = browser.new_context(viewport={"width": 1280, "height": 720})
        ctx.add_init_script(script)
        p = ctx.new_page()
        errors = []
        p.on("pageerror", lambda e: errors.append(str(e)))
        p.goto(pathlib.Path(a.page).resolve().as_uri() + query)
        if a.dualsense:
            # The pad in use from the start (a nudge of the stick every few seconds), while the
            # first waves walk in and hit.
            for _ in range(10):
                p.wait_for_timeout(6000)
                p.evaluate("window.__pad.axes[0] = 0.5")
                p.wait_for_timeout(1500)
                p.evaluate("window.__pad.axes[0] = 0")
        else:
            p.wait_for_timeout(30000)
        # A stick push makes the pad the device in use: the prompts turn to its buttons.
        p.evaluate("window.__pad.axes[0] = 0.6")
        p.wait_for_timeout(3000)
        p.evaluate("window.__pad.axes[0] = 0")
        p.wait_for_timeout(2500)
        p.screenshot(path=str(out / "1-hud.png"))
        if a.dualsense:
            # Stand in the arena while a wave comes in and hits.
            p.wait_for_timeout(30000)
            p.screenshot(path=str(out / "2-fight.png"))
            press(p, 17)  # The touchpad's press: the map.
            p.screenshot(path=str(out / "3-touchpad-map.png"))
            calls = p.evaluate("window.__calls")
            summary = {
                "light_colours": sorted(set(c for c in calls["light"] if c)),
                "light_calls": len(calls["light"]),
                "trigger_effects": sorted(set(json.dumps(t[1]) for t in calls["trigger"])),
                "rumbles": len(calls["rumble"]),
                "microphone_requests": calls["mic"],
            }
            (out / "dualsense.json").write_text(json.dumps(summary, indent=2))
            print(json.dumps(summary))
        else:
            press(p, 9)  # Options: the pause menu.
            p.screenshot(path=str(out / "2-menu.png"))
            dpad_down(p, a.unmapped)  # D-pad down: the next item.
            p.screenshot(path=str(out / "3-down.png"))
            press(p, 0)  # Cross: take it.
            p.wait_for_timeout(2500)
            p.screenshot(path=str(out / "4-settings.png"))
        print("page errors:", errors[:3] if errors else "none")
        browser.close()


if __name__ == "__main__":
    main()
