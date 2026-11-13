// The AI Hub's three screenshots of Shardfall (AI Hub Catalogue v1, docs/AI_HUB.md): the finished
// page in headless Chromium (WebGPU), outside a session, in its own language, nothing over it
// that isn't the game's. A 960 x 720 window at a device scale factor of 2/3, JPEG quality 58,
// gives the hub's 640 x 480 baseline frames.
//
//   node scripts/hub-covers.js PAGE.html OUT_DIR
//
// 0.jpg: the opening view, the Pavilion's plaza. 1.jpg, 2.jpg: play (the Proving Grounds' waves, a
// descent level).
// Software rendering is slow (a frame takes about a second), so each moment is given time.
const { chromium } = require('playwright');
const path = require('path');

const [page, out] = process.argv.slice(2);
if (!page || !out) {
  console.error('usage: node scripts/hub-covers.js PAGE.html OUT_DIR');
  process.exit(2);
}
const url = 'file://' + path.resolve(page);

async function shot(browser, query, file, act) {
  const ctx = await browser.newContext({ viewport: { width: 960, height: 720 }, deviceScaleFactor: 2 / 3 });
  const p = await ctx.newPage();
  const errors = [];
  p.on('pageerror', (e) => errors.push(e.message));
  p.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  await p.goto(url + query);
  await p.waitForTimeout(25000);
  await act(p);
  await p.screenshot({ path: path.join(out, file), type: 'jpeg', quality: 58, timeout: 180000 });
  console.log(`${file}${errors.length ? ' (console: ' + errors.slice(0, 3).join('; ') + ')' : ''}`);
  await ctx.close();
}

const hold = async (p, code, ms) => {
  await p.keyboard.down(code);
  await p.waitForTimeout(ms);
  await p.keyboard.up(code);
};

(async () => {
  const browser = await chromium.launch({
    args: ['--enable-unsafe-webgpu', '--enable-features=Vulkan', '--use-vulkan=swiftshader', '--use-angle=swiftshader'],
  });
  // The opening view: the splash skipped with a click, the camera down from over the plaza to
  // the player, with the welcome and the studios' and tools' names.
  await shot(browser, '', '0.jpg', async (p) => {
    await p.mouse.click(480, 360);
    await p.waitForTimeout(25000);
  });
  // The Proving Grounds: a wave closes in and the hero swings and casts.
  await shot(browser, '?scene=arena&seed=3', '1.jpg', async (p) => {
    await p.mouse.move(600, 300);
    await p.waitForTimeout(20000);
    for (let k = 0; k < 6; k++) {
      await p.mouse.down();
      await p.waitForTimeout(1500);
      await p.mouse.up();
      await hold(p, k % 2 ? 'KeyR' : 'KeyQ', 600);
    }
    await p.waitForTimeout(4000);
  });
  // A descent level: down the first corridor toward the first room.
  await shot(browser, '?scene=level/6&seed=5', '2.jpg', async (p) => {
    await p.mouse.move(480, 200);
    await hold(p, 'KeyW', 14000);
    await p.mouse.down();
    await p.waitForTimeout(2500);
    await p.mouse.up();
    await p.waitForTimeout(3000);
  });
  await browser.close();
})();
