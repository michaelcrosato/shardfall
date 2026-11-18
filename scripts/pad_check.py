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
  extras allowed, Motion aim on, the microphone allowed) put in the page as the hub's companion
  would: in the Proving Grounds the hero is hit, and the light bar's colours, the triggers'
  effects and the rumbles are recorded (OUT_DIR/dualsense.json); the touchpad's press opens the
  map. Then motion: the controller turns left at a steady 22.5 degrees a second for 4 seconds
  (90 degrees) while Square attacks, so the hero's attacks swing round (counter-clockwise on
  screen: 180 degrees at the Medium sensitivity), screenshotted before and after; reads of the
  gyro are counted. After the fight D-pad up drinks a potion: the speaker's cue. openSpeaker()
  is a spy giving a silent node: its calls, the cues the game asked for and the nodes connected
  into it are counted.
  Shout to roar: the hero is a new one of the character whose starting bar holds War Cry
  (game/heroes.toml: Kestrel, its fifth button), put in the page's save before the game starts,
  since the page can't change the bar. openMicrophone() gives a stand-in microphone, a 220 Hz
  tone from an OscillatorNode through a MediaStreamAudioDestinationNode, loud (0.8) for 0.5 s
  every 4 s and silent between, from two seconds after the call (the page listens 1.5 s before
  it counts a shout). Recorded: the calls to openMicrophone() (1: the game asks once) and to
  getUserMedia() (0: never), the tone's bursts so far, the shouts the page's
  shardfallHub.shout() reported (about one a burst: none lost, even at a frame a second) and
  the War Cries they cast (shardfallHub.roars, counted once War Cry went off: at least 1, at
  most one each 12 s cooldown; a shout while mana is short, the hero dead or War Cry cooling
  down does nothing). 6-roar.png is taken as one goes off (its orange ring; "Roar!" at the top).
"""
import argparse
import json
import pathlib
import tomllib

from playwright.sync_api import sync_playwright

FLAGS = ["--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-vulkan=swiftshader", "--use-angle=swiftshader"]
ROOT = pathlib.Path(__file__).resolve().parent.parent


def roar_save():
    """A new hero of the first character (in the hall's order) whose starting bar holds War Cry,
    as the game's save (localStorage `shardfall_save`, save.rs): its key, War Cry's slot on the
    bar (0-5) and the save's JSON."""
    heroes = tomllib.loads((ROOT / "game" / "heroes.toml").read_text())
    items = tomllib.loads((ROOT / "game" / "items.toml").read_text())
    chars = sorted((c.get("order", 99), k, c) for k, c in heroes.items() if isinstance(c, dict))
    for _, key, c in chars:
        bar = c.get("bar", [])
        if "war_cry" not in bar:
            continue
        equipment = [None] * 10
        weapon = c.get("weapon")
        if weapon in items:
            name = items[weapon].get("name", weapon)
            equipment[0] = {"id": 1, "base": weapon, "level": 1, "rarity": "normal", "name": name}
        hero = {"name": c.get("name", key), "character": key, "bar": bar, "equipment": equipment, "next_item": 2}
        return key, bar.index("war_cry"), json.dumps({"version": 1, "hero": hero})
    raise SystemExit("no character starts with war_cry on its bar (game/heroes.toml)")


PAD = """
(() => {
  const unmapped = __UNMAPPED__;
  const state = { id: __ID__, buttons: new Array(unmapped ? 12 : 18).fill(0),
                  axes: unmapped ? [0, 0, 0, 0, 0, 0, 0, 0, 0, 1.2857] : [0, 0, 0, 0] };
  window.__pad = state;
  window.__calls = { rumble: [], light: [], trigger: [], getUserMedia: 0, openMicrophone: 0, shouts: 0, gyroReads: 0,
                     speakerOpens: 0, speakerConnects: 0, cues: [] };
  state.gyro = { x: 0, y: 0, z: 0 };
  // What the game connects into the speaker's node (one gain per cue).
  if (window.AudioNode) {
    const connect = AudioNode.prototype.connect;
    AudioNode.prototype.connect = function (to, ...rest) {
      if (to && to === window.__speakerNode) window.__calls.speakerConnects++;
      return connect.call(this, to, ...rest);
    };
  }
  const pad = () => ({
    id: state.id, index: 0, connected: true, mapping: unmapped ? '' : 'standard', timestamp: performance.now(),
    axes: state.axes.slice(),
    buttons: state.buttons.map((v) => ({ pressed: v > 0.5, touched: v > 0, value: v })),
    vibrationActuator: { type: 'dual-rumble', playEffect: (kind, p) => { window.__calls.rumble.push(p); return Promise.resolve('complete'); } },
  });
  Object.defineProperty(navigator, 'getGamepads', { configurable: true, value: () => [pad(), null, null, null] });
  if (navigator.mediaDevices) {
    navigator.mediaDevices.getUserMedia = () => { window.__calls.getUserMedia++; return Promise.reject(new Error('no')); };
  }
  if (__DUALSENSE__) {
    // A hero with War Cry on the bar (the page can't change the bar): the game loads the save.
    try { if (!localStorage.getItem('shardfall_save')) localStorage.setItem('shardfall_save', __SAVE__); } catch (e) {}
    // 52in1's player settings, as its companion script gives every game page: a DualSense with
    // its extras allowed and the player's Motion aim on, and the microphone allowed.
    window.AIHubSettings = {
      music: 1, effects: 1, vibration: 1, mic: true, micLater: false, muted: false,
      dualsense: {
        connected: true, lightbar: true, triggers: true, motion: true, touchpad: true, speaker: true, aim: true,
        get gyro() { window.__calls.gyroReads++; return { ...state.gyro }; },
        accel: { x: 0, y: 1, z: 0 }, touches: [],
        light: (c) => window.__calls.light.push(c),
        trigger: (w, e) => window.__calls.trigger.push([w, e]),
        // The speaker: a silent node in the game's context.
        openSpeaker: async (ctx) => {
          window.__calls.speakerOpens++;
          const node = ctx.createGain();
          node.gain.value = 0;
          window.__speakerNode = node;
          return node;
        },
      },
      // The microphone: a 220 Hz tone, loud for 0.5 s every 4 s (a shout), silent between.
      openMicrophone: async () => {
        window.__calls.openMicrophone++;
        const ctx = window.__micCtx || (window.__micCtx = new (window.AudioContext || window.webkitAudioContext)());
        if (ctx.state === 'suspended') await ctx.resume().catch(() => {});
        const osc = ctx.createOscillator(), gain = ctx.createGain(), out = ctx.createMediaStreamDestination();
        osc.frequency.value = 220;
        gain.gain.value = 0;
        const t0 = ctx.currentTime + 2;
        window.__micStart = t0;
        for (let i = 0; i < 150; i++) {
          gain.gain.setValueAtTime(0.8, t0 + i * 4);
          gain.gain.setValueAtTime(0, t0 + i * 4 + 0.5);
        }
        osc.connect(gain).connect(out);
        osc.start();
        return out.stream;
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
    roar_key, roar_slot, save = roar_save() if a.dualsense else ("", -1, "")
    script = script.replace("__SAVE__", json.dumps(save))
    # The audio contexts (the page's, for the speaker and the microphone's listening, and the
    # stand-in microphone's) run with no click or key press first.
    flags = FLAGS + (["--autoplay-policy=no-user-gesture-required"] if a.dualsense else [])
    with sync_playwright() as pw:
        browser = pw.chromium.launch(args=flags)
        ctx = browser.new_context(viewport={"width": 1280, "height": 720})
        ctx.add_init_script(script)
        p = ctx.new_page()
        errors = []
        p.on("pageerror", lambda e: errors.append(str(e)))
        p.goto(pathlib.Path(a.page).resolve().as_uri() + query)
        if a.dualsense:
            # The cues the game asks the page's speaker for.
            p.evaluate(
                "() => { const f = shardfallHub.speaker;"
                " shardfallHub.speaker = (cue, volume) => { window.__calls.cues.push(cue); return f(cue, volume); }; }"
            )
            # The shouts the page heard and handed the game.
            p.evaluate(
                "() => { const f = shardfallHub.shout;"
                " shardfallHub.shout = (on) => { const s = f(on); if (s) window.__calls.shouts++; return s; }; }"
            )
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
            press(p, 12)  # D-pad up: a potion while the hero is hurt, the speaker's cue.
            press(p, 17)  # The touchpad's press: the map.
            p.screenshot(path=str(out / "3-touchpad-map.png"))
            press(p, 17)  # And again: the map closes.
            # Motion aim: Square attacks while the controller turns left, 90 degrees in 4 s.
            reads_before = p.evaluate("window.__calls.gyroReads")
            p.evaluate("window.__pad.buttons[2] = 1")
            p.wait_for_timeout(2500)
            p.screenshot(path=str(out / "4-motion-before.png"))
            p.evaluate("window.__pad.gyro.y = 22.5")
            p.wait_for_timeout(4000)
            p.evaluate("window.__pad.gyro.y = 0")
            p.wait_for_timeout(1500)
            p.screenshot(path=str(out / "5-motion-turned.png"))
            p.evaluate("window.__pad.buttons[2] = 0")
            p.wait_for_timeout(1500)
            # Shout to roar: the next War Cry a shout casts (its ring, "Roar!" at the top).
            roars = p.evaluate("shardfallHub.roars")
            for _ in range(60):
                if p.evaluate("shardfallHub.roars") > roars:
                    break
                p.wait_for_timeout(500)
            p.screenshot(path=str(out / "6-roar.png"))
            calls = p.evaluate("window.__calls")
            bursts = p.evaluate(
                "window.__micCtx && window.__micStart !== undefined"
                " ? Math.max(0, Math.floor((window.__micCtx.currentTime - window.__micStart) / 4) + 1) : 0"
            )
            summary = {
                "light_colours": sorted(set(c for c in calls["light"] if c)),
                "light_calls": len(calls["light"]),
                "trigger_effects": sorted(set(json.dumps(t[1]) for t in calls["trigger"])),
                "rumbles": len(calls["rumble"]),
                "gyro_reads": calls["gyroReads"],
                "gyro_reads_while_turning": calls["gyroReads"] - reads_before,
                "motion_turn_degrees": 90,
                "speaker_opens": calls["speakerOpens"],
                "speaker_cues": calls["cues"],
                "speaker_nodes_connected": calls["speakerConnects"],
                "roar_hero": roar_key,
                "roar_button": roar_slot + 1,
                "open_microphone_calls": calls["openMicrophone"],
                "get_user_media_calls": calls["getUserMedia"],
                "mic_bursts": bursts,
                "shouts_reported": calls["shouts"],
                "war_cries_cast": p.evaluate("shardfallHub.roars"),
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
